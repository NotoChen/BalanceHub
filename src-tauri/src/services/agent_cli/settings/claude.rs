use super::*;
mod context;

fn model_setting(snapshot: &NativeSnapshot, model: &str, key: &str) -> Option<Observed> {
    let path = format!("/modelSettings/{}/{}", native_model_pointer(model), key);
    snapshot
        .layers
        .iter()
        .rev()
        .filter(|layer| !layer.requirements)
        .find_map(|layer| {
            snapshot.value_from_layer(layer, &path).or_else(|| {
                // Current Claude releases ignore the legacy user-wide effort on
                // 5.5 models, while project/local/managed effort still applies.
                if key == "effortLevel"
                    && layer.scope == AgentAssetScope::User
                    && (model.contains("-5-5") || model.contains("-5.5"))
                {
                    None
                } else {
                    snapshot.value_from_layer(layer, &format!("/{key}"))
                }
            })
        })
}

pub(super) fn fields(
    snapshot: &NativeSnapshot,
    model: &str,
    help: &str,
    persistent: bool,
) -> Vec<BoundSetting> {
    let mut fields = Vec::new();
    let model_window = context::window(snapshot, model);
    let model_lower = model.to_ascii_lowercase();
    let model_is_46 = ["opus-4-6", "opus-4.6", "sonnet-4-6", "sonnet-4.6"]
        .iter()
        .any(|id| model_lower.contains(id));
    if persistent || help.contains("--effort") {
        let effort_help = help_option(help, "--effort");
        let mut efforts = vec![("low", "低", ""), ("medium", "中", ""), ("high", "高", "")];
        if ((persistent && help.is_empty()) || effort_help.contains("xhigh")) && !model_is_46 {
            efforts.push(("xhigh", "更高", ""));
        }
        if (persistent && help.is_empty()) || effort_help.contains("max") {
            efforts.push(("max", "最大", "本次会话使用的最深推理档位"));
        }
        let mut field = BoundSetting::new("effort", "model", "推理强度", "CLAUDE_CODE_EFFORT_LEVEL / --effort", Binding::Env("CLAUDE_CODE_EFFORT_LEVEL"))
            .choices(&efforts).describe("按本机 CLI 帮助提供档位，并按已识别的模型收敛选项。模型不支持的档位和管理上限仍由 Claude 检查；Ultracode 是工作流开关，不混作推理强度。")
            .current(snapshot.env("CLAUDE_CODE_EFFORT_LEVEL", Some("/env/CLAUDE_CODE_EFFORT_LEVEL"))
                .or_else(|| model_setting(snapshot, model, "effortLevel")),
                "未设置")
            .unset_detail("未设置", "没有设置推理强度，当前也没有取得此模型的明确默认档位。选择具体模型或手动指定档位后可确定本次设置。");
        if model.contains("-5-5") || model.contains("-5.5") {
            field = field.default(json!("medium"), "Claude 5.5 模型默认推理强度");
        } else if model.contains("opus-4-7") || model.contains("opus-4.7") {
            field = field.default(json!("xhigh"), "Claude Opus 4.7 默认推理强度");
        }
        let rank = |value: &str| {
            ["low", "medium", "high", "xhigh", "max"]
                .iter()
                .position(|level| *level == value)
        };
        let cap = snapshot
            .layers
            .iter()
            .filter(|layer| !layer.requirements)
            .filter_map(|layer| {
                layer
                    .value
                    .pointer(&format!(
                        "/modelSettings/{}/maxEffortLevel",
                        native_model_pointer(model)
                    ))
                    .or_else(|| layer.value.get("maxEffortLevel"))
                    .and_then(Value::as_str)
                    .and_then(rank)
            })
            .min();
        if let Some(cap) = cap {
            field
                .ui
                .choices
                .retain(|choice| rank(&choice.value).is_some_and(|rank| rank <= cap));
            field
                .ui
                .description
                .push_str(" 已按读取到的 maxEffortLevel 上限过滤。");
            if let Some(current) = field.ui.current_value.as_deref() {
                if rank(current).is_some_and(|value| value > cap) {
                    if let Some(effective) = field.ui.choices.last() {
                        field.ui.current_label = format!(
                            "{} · {}（原配置 {}，受管理上限限制）",
                            effective.label, effective.value, current
                        );
                        field.ui.current_value = Some(effective.value.clone());
                    }
                }
            }
        }
        fields.push(field);
    }
    let recognized_name = model_lower.contains("claude-")
        || ["opus", "sonnet", "haiku", "fable"].contains(&model_lower.as_str());
    let context_override_applies =
        !model.is_empty() && !recognized_name && !model_lower.contains("[1m]");
    let mut context = BoundSetting::new("contextWindow", "model", "上下文窗口", "CLAUDE_CODE_MAX_CONTEXT_TOKENS", Binding::Env("CLAUDE_CODE_MAX_CONTEXT_TOKENS"))
        .number(1, None, "tokens")
        .describe("此变量用于 Claude 无法识别的网关模型别名。已识别 Claude 模型的窗口由模型和账号决定；自动压缩阈值可独立调整，不会扩大服务端容量。")
        .current(context_override_applies.then(|| snapshot.env("CLAUDE_CODE_MAX_CONTEXT_TOKENS", Some("/env/CLAUDE_CODE_MAX_CONTEXT_TOKENS"))).flatten(),
            "未设置")
        .unset_detail("未设置", "尚未识别到此模型的具体容量。模型别名、账号权限和服务端配置可能影响窗口；请求 1M 上下文也需要服务端支持。");
    if let Some(window) = &model_window {
        context = context.default(
            json!(window.capacity),
            "Claude 官方模型窗口；服务端与账号限制仍适用",
        );
        context.ui.current_label = format!("{} tokens", window.capacity);
    }
    let capacity = context
        .ui
        .current_value
        .as_deref()
        .and_then(|value| value.parse::<i64>().ok());
    fields.push(
        if model.is_empty() || recognized_name || model_lower.contains("[1m]") {
            context
                .readonly()
                .locked("当前模型不能单独用此变量覆盖窗口；请用下方自动压缩阈值控制本次会话规模")
        } else {
            context
        },
    );
    if persistent || help.contains("--autocompact") {
        let mut compact = BoundSetting::new("autoCompact", "model", "自动压缩窗口", "CLAUDE_CODE_AUTO_COMPACT_WINDOW / --autocompact", Binding::Env("CLAUDE_CODE_AUTO_COMPACT_WINDOW"))
            .number(100_000, Some(1_000_000), "tokens")
            .percentage_base(capacity)
            .describe("原生支持 100K–1M tokens，实际不超过模型窗口。本次通过环境变量设置，避免被已有环境变量盖过。")
            .current(snapshot.env("CLAUDE_CODE_AUTO_COMPACT_WINDOW", Some("/env/CLAUDE_CODE_AUTO_COMPACT_WINDOW"))
                .or_else(|| model_setting(snapshot, model, "autoCompactWindow")), "未设置")
            .unset_detail("未设置", "没有设置压缩阈值，当前模型的默认窗口也未确定，暂时无法显示具体 token 数。");
        if compact.ui.current_value.is_none() || compact.ui.current_value.as_deref() == Some("auto")
        {
            if let Some(window) = &model_window {
                let native = compact.ui.current_native_value.take();
                compact.ui.current_value = None;
                compact = compact.default(json!(window.compact), "Claude 官方自动压缩默认值");
                compact.ui.current_native_value = native;
                if window.approximate {
                    compact.ui.current_label = format!("约 {} tokens", window.compact);
                    compact.ui.hint =
                        "原生 1M 模型默认约在 967K tokens 压缩，具体触发时机由 Claude 管理。"
                            .into();
                }
            } else {
                compact.ui.current_value = None;
                compact = compact.unset_detail(
                    "未设置",
                    "已选择自动压缩，但当前模型的默认阈值尚未取得；可以明确指定 token 数。",
                );
            }
        }
        fields.push(compact);
    }
    fields.push(
        BoundSetting::new(
            "outputLimit",
            "model",
            "单次输出上限",
            "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
            Binding::Env("CLAUDE_CODE_MAX_OUTPUT_TOKENS"),
        )
        .number(1, None, "tokens")
        .describe("限制一轮回答的输出 token 数；思考与输出的服务端上限仍由模型决定。")
        .current(
            snapshot.env(
                "CLAUDE_CODE_MAX_OUTPUT_TOKENS",
                Some("/env/CLAUDE_CODE_MAX_OUTPUT_TOKENS"),
            ),
            "未设置",
        )
        .unset_detail(
            "未设置",
            "未设置 CLAUDE_CODE_MAX_OUTPUT_TOKENS，当前也未取得所选模型的默认输出上限。",
        ),
    );

    if persistent || help.contains("--permission-mode") {
        let manual = if help.contains("\"manual\"") {
            "manual"
        } else {
            "default"
        };
        let mut choices = vec![(manual, "逐项确认", "文件读取之外的操作按原生权限规则确认")];
        for option in [
            (
                "acceptEdits",
                "自动接受编辑",
                "编辑与常见文件操作免确认，其他操作按规则处理",
            ),
            ("plan", "规划模式", "分析并制定计划，批准计划后再编辑"),
            (
                "auto",
                "自动审查",
                "由 Claude 的审查模型评估操作；可用性由模型和账号决定",
            ),
            ("dontAsk", "只用已授权权限", "需要弹窗确认的操作自动拒绝"),
            (
                "bypassPermissions",
                "跳过权限询问",
                "不会绕过组织策略和显式拒绝规则",
            ),
        ] {
            if (persistent && help.is_empty()) || help.contains(&format!("\"{}\"", option.0)) {
                choices.push(option);
            }
        }
        if snapshot.layers.iter().any(|layer| {
            layer
                .value
                .pointer("/permissions/disableBypassPermissionsMode")
                == Some(&json!("disable"))
        }) {
            choices.retain(|choice| choice.0 != "bypassPermissions");
        }
        if snapshot.layers.iter().any(|layer| {
            layer.value.get("disableAutoMode") == Some(&json!("disable"))
                || layer.value.pointer("/permissions/disableAutoMode") == Some(&json!("disable"))
        }) {
            choices.retain(|choice| choice.0 != "auto");
        }
        let current = snapshot
            .layers
            .iter()
            .rev()
            .filter_map(|layer| {
                let value = snapshot.value_from_layer(layer, "/permissions/defaultMode")?;
                if matches!(
                    layer.scope,
                    AgentAssetScope::Workspace | AgentAssetScope::Local
                ) && matches!(value.value.as_str(), Some("auto" | "bypassPermissions"))
                {
                    None
                } else {
                    Some(value)
                }
            })
            .next()
            .map(|mut value| {
                if value.value == "default" && manual == "manual" {
                    value.value = json!("manual");
                }
                value
            });
        fields.push(BoundSetting::new("approval", "permissions", "操作审批", "--permission-mode / permissions.defaultMode", Binding::Flag("--permission-mode"))
            .choices(&choices).current(current, "未设置")
            .unset_detail("未设置", "未设置启动审批模式。当前 Claude 版本可能根据账号与自动审查可用性选择 auto 或 manual，尚未取得实际结果。")
            .describe("按原生优先级读取启动模式；项目和本地文件里的 auto、bypassPermissions 不作为终端启动默认值。未配置时还取决于 CLI 版本、账号和自动审查可用性。"));
    }
    if persistent || help.contains("--settings") {
        let observed = snapshot.get("/sandbox/enabled");
        let managed = observed.as_ref().is_some_and(|value| value.managed);
        let field = BoundSetting::new(
            "sandbox",
            "permissions",
            "命令沙盒",
            "--settings → sandbox.enabled",
            Binding::Json("/sandbox/enabled", ValueType::Boolean),
        )
        .choices(&boolean())
        .describe("为 Bash 提供文件系统与网络隔离；与审批模式独立。需要本机原生沙盒运行时支持。")
        .current(observed, "关闭 · false")
        .default(json!(false), "Claude 内置默认值");
        fields.push(if managed {
            field.locked("此沙盒设置来自原生管理配置")
        } else {
            field
        });
        fields.push(
            BoundSetting::new(
                "askTools",
                "permissions",
                "始终确认的工具规则",
                "--settings → permissions.ask",
                Binding::JsonAppend("/permissions/ask"),
            )
            .rules()
            .describe("每行一条原生规则，例如 Bash(git push *)。追加到当前规则，不清除已有约束。")
            .current(rule_current(snapshot, "/permissions/ask"), "未添加此类规则")
            .unset("未添加此类规则"),
        );
    }
    if persistent || help.contains("--tools") {
        fields.push(BoundSetting::new("tools", "tools", "本次可用的内置工具", "--tools", Binding::FlagList("--tools"))
            .list().custom().choices(&[
                ("Read", "读取文件", ""), ("Edit", "编辑文件", ""), ("Write", "写入文件", ""), ("Bash", "执行命令", ""),
                ("Glob", "查找文件", "部分平台默认不加载，可显式选择"), ("Grep", "搜索内容", "部分平台默认不加载，可显式选择"),
                ("WebSearch", "网页搜索", ""), ("WebFetch", "读取网页", ""), ("Agent", "子 Agent", ""),
                ("Skill", "调用 Skill", ""), ("NotebookEdit", "编辑 Notebook", ""), ("LSP", "代码智能", "需要原生 LSP 配置"),
                ("TaskCreate", "创建任务", "按模型与版本提供"), ("TaskGet", "查看任务", ""), ("TaskList", "任务列表", ""), ("TaskUpdate", "更新任务", ""),
            ]).describe("自选后仅向模型提供选中的内置工具；不选任何工具会传入空工具集。MCP 在资产页管理，原生 deny 规则仍会限制调用。可补充本机版本支持的其他工具名。")
            .current(None, "未额外限制内置工具")
            .unset("未额外限制内置工具"));
    }
    for (id, flag, pointer, label) in [
        (
            "allowTools",
            "--allowed-tools",
            "/permissions/allow",
            "免确认的工具规则",
        ),
        (
            "denyTools",
            "--disallowed-tools",
            "/permissions/deny",
            "禁止的工具规则",
        ),
    ] {
        if persistent || help.contains(flag) {
            fields.push(BoundSetting::new(id, "permissions", label, flag, Binding::FlagRules(flag)).rules()
                .describe("每行一条原生规则。可限定命令、路径或工具，如 Bash(git status *)、Read(src/**)。这与工具是否出现在模型工具集中分别生效。")
                .current(rule_current(snapshot, pointer), "未添加此类规则").unset("未添加此类规则"));
        }
    }
    fields
}

pub(super) fn rule_current(snapshot: &NativeSnapshot, pointer: &str) -> Option<Observed> {
    let values = snapshot.arrays(pointer);
    let sources = snapshot
        .layers
        .iter()
        .filter(|layer| {
            !layer.requirements && layer.value.pointer(pointer).is_some_and(Value::is_array)
        })
        .map(|layer| {
            crate::platform::paths::compatible_path(&layer.path)
                .to_string_lossy()
                .into_owned()
        })
        .collect::<std::collections::BTreeSet<_>>();
    (!sources.is_empty()).then(|| Observed {
        value: json!(values),
        source: sources.into_iter().collect::<Vec<_>>().join("\n"),
        managed: false,
        state: AgentSettingValueState::Configured,
    })
}
