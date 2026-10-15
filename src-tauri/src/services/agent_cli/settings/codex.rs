use super::*;

mod catalog;
mod collaboration;
mod context;
mod features;
mod generation;
pub(super) use catalog::{
    configured_path as configured_catalog_path, query as query_model_catalog,
};
pub(super) use collaboration::{v2_child_count, v2_total_count, V2_LIMIT_KEY, V2_LIMIT_POINTER};
pub(super) use features::load as load_features;

pub(super) fn fields(
    snapshot: &NativeSnapshot,
    model: &str,
    help: &str,
    cli_path: &str,
    persistent: bool,
    native_options: Option<&std::collections::BTreeMap<String, String>>,
) -> Vec<BoundSetting> {
    let mut fields = Vec::new();
    let model_info = catalog::read(snapshot, cli_path, model);
    let model_value = |key: &str| model_info.as_ref().and_then(|info| info.observed(key));
    if persistent || help.contains("--config") {
        let effort = generation::effort(
            BoundSetting::new("effort", "model", "推理强度", "-c model_reasoning_effort", Binding::Codex("model_reasoning_effort", ValueType::Text))
                .describe("推理档位由所选模型声明。同名档位在不同模型上不代表相同计算量；未读取到模型目录时可填写服务端支持的原生值。"),
            model_info.as_ref(),
        );
        fields.push(
            effort
                .current(
                    snapshot
                        .get("/model_reasoning_effort")
                        .or_else(|| model_value("default_reasoning_level")),
                    "未设置",
                )
                .unset_detail(
                    "未设置",
                    "未设置 model_reasoning_effort，当前 CLI 模型目录也没有返回默认档位。",
                ),
        );
        fields.extend(context::fields(
            snapshot,
            model_info.as_ref(),
            native_options,
        ));
        fields.extend(generation::fields(snapshot, model, model_info.as_ref()));
    }
    if persistent || help.contains("--sandbox") {
        fields.push(
            BoundSetting::new(
                "sandbox",
                "permissions",
                "文件访问范围",
                "--sandbox / sandbox_mode",
                Binding::Flag("--sandbox"),
            )
            .choices(&[
                ("read-only", "只读", "沙盒内只读；写入需额外授权"),
                (
                    "workspace-write",
                    "项目内可写",
                    "工作目录和明确添加的可写目录",
                ),
                (
                    "danger-full-access",
                    "完全访问",
                    "不使用文件系统沙盒；仍遵循所选审批策略",
                ),
            ])
            .describe("文件访问和操作审批独立设置。附加目录会扩展项目可写沙盒的工作范围。")
            .current(snapshot.get("/sandbox_mode"), "未设置")
            .unset_detail("未设置", "未设置 sandbox_mode；Codex 还会结合项目信任状态和权限方案选择文件访问范围，当前未取得最终结果。")
            .restrict(snapshot.requirement("/allowed_sandbox_modes")),
        );
    }
    if persistent || help.contains("--ask-for-approval") {
        let mut choices = vec![
            ("on-request", "按需确认", "模型需要额外权限时提出申请"),
            ("never", "不询问", "需要审批的操作直接失败，不会自动放权"),
        ];
        if persistent || help.contains("--config") {
            choices.push((
                "granular",
                "按类别确认",
                "分别允许或自动拒绝不同类别的审批请求",
            ));
        }
        let mut approval = BoundSetting::new(
            "approval",
            "permissions",
            "操作审批",
            "-c approval_policy",
            Binding::CodexApproval,
        )
        .choices(&choices)
        .current(snapshot.get("/approval_policy"), "按需确认 · on-request")
        .restrict(snapshot.requirement("/allowed_approval_policies"));
        if approval.ui.current_value.is_none() && snapshot.trust == AgentTrustState::Untrusted {
            approval = approval.unset_detail(
                "按未信任目录的规则确认",
                "没有设置 approval_policy，所选目录尚未信任；启动时 Codex 会按目录信任状态确认操作，当前未取得最终审批策略。",
            );
            approval.ui.source = "Codex 根据目录信任状态选择".into();
        } else {
            approval = approval.default(json!("on-request"), "Codex 内置默认值");
        }
        let granular = snapshot.get("/approval_policy/granular");
        if granular.is_some() {
            approval.ui.current_value = Some("granular".into());
            approval.ui.current_label = "按类别确认 · granular".into();
        }
        fields.push(approval);
        if persistent || help.contains("--config") {
            fields.push(
                BoundSetting::new(
                    "approvalCategories",
                    "permissions",
                    "允许弹出确认的类别",
                    "-c approval_policy.granular",
                    Binding::CodexGranular,
                )
                .list()
                .when("approval", "granular")
                .choices(&[
                    ("sandbox_approval", "沙盒权限申请", ""),
                    ("rules", "命令规则确认", ""),
                    ("mcp_elicitations", "MCP 交互确认", ""),
                    ("request_permissions", "工具申请权限", ""),
                    ("skill_approval", "Skill 脚本确认", ""),
                ])
                .describe("勾选的类别可以请求你确认；未勾选的类别会被自动拒绝，不会直接执行。")
                .current(
                    granular.map(|mut observed| {
                        observed.value = json!([
                            "sandbox_approval",
                            "rules",
                            "mcp_elicitations",
                            "request_permissions",
                            "skill_approval"
                        ]
                        .into_iter()
                        .filter(|key| observed.value.get(*key) != Some(&json!(false)))
                        .collect::<Vec<_>>());
                        observed
                    }),
                    "未启用按类别确认",
                )
                .unset("未启用按类别确认"),
            );
        }
    }
    if persistent || help.contains("--config") {
        if persistent || help.contains("--approve-for-me") {
            fields.push(
                BoundSetting::new(
                    "approvalReviewer",
                    "permissions",
                    "审批处理方式",
                    "-c approvals_reviewer",
                    Binding::Codex("approvals_reviewer", ValueType::Text),
                )
                .choices(&[
                    ("user", "由我确认", ""),
                    (
                        "auto_review",
                        "原生自动审查",
                        "交由 Codex 审查请求；不改变文件沙盒范围",
                    ),
                ])
                .current(snapshot.get("/approvals_reviewer"), "由我确认 · user")
                .default(json!("user"), "Codex 内置默认值")
                .restrict(snapshot.requirement("/allowed_approvals_reviewers")),
            );
        }
        fields.push(
            BoundSetting::new(
                "sandboxNetwork",
                "permissions",
                "沙盒内网络访问",
                "-c sandbox_workspace_write.network_access",
                Binding::Codex("sandbox_workspace_write.network_access", ValueType::Boolean),
            )
            .choices(&boolean())
            .describe("只作用于项目可写沙盒中的命令网络访问；网页搜索和 MCP 使用各自的连接机制。")
            .current(
                snapshot.get("/sandbox_workspace_write/network_access"),
                "关闭 · false",
            )
            .default(json!(false), "Codex 内置默认值"),
        );
        fields.push(
            BoundSetting::new(
                "webSearch",
                "tools",
                "网页搜索",
                "-c web_search",
                Binding::Codex("web_search", ValueType::Text),
            )
            .choices(&[
                ("disabled", "停用", ""),
                ("cached", "缓存搜索", "优先使用已缓存内容"),
                (
                    "indexed",
                    "索引搜索",
                    "需要支持 indexed 的 Codex 版本与服务",
                ),
                ("live", "实时搜索", "请求最新网页结果"),
            ])
            .current(snapshot.get("/web_search"), "未设置")
            .default(
                json!(if snapshot
                    .get("/sandbox_mode")
                    .is_some_and(|value| value.value == "danger-full-access")
                    || snapshot
                        .get("/default_permissions")
                        .is_some_and(|value| value.value == ":danger-full-access")
                {
                    "live"
                } else {
                    "cached"
                }),
                "Codex 内置默认值（按文件访问范围选择）",
            ),
        );
        fields.push(
            BoundSetting::new(
                "shell",
                "tools",
                "命令执行工具",
                "-c features.shell_tool",
                Binding::Codex("features.shell_tool", ValueType::Boolean),
            )
            .choices(&boolean())
            .describe("控制命令执行工具是否提供给模型；执行时仍遵循沙盒和审批设置。")
            .current(
                snapshot
                    .get("/features/shell_tool")
                    .or_else(|| snapshot.feature_default("shell_tool")),
                "启用 · true",
            )
            .default(json!(true), "Codex 内置默认值"),
        );
        fields.extend(collaboration::fields(
            snapshot,
            model_info.as_ref(),
            cli_path,
            native_options,
        ));
        fields.push(
            BoundSetting::new(
                "loginShell",
                "tools",
                "登录 Shell",
                "-c allow_login_shell",
                Binding::Codex("allow_login_shell", ValueType::Boolean),
            )
            .choices(&boolean())
            .describe("允许命令工具加载登录 Shell 的初始化文件。关闭后，省略 login 的命令使用普通 Shell，明确请求登录 Shell 的命令会被拒绝。")
            .current(snapshot.get("/allow_login_shell"), "启用")
            .default(json!(true), "Codex 内置默认值"),
        );
        for (id, label, key) in [
            ("imageGeneration", "图像生成工具", "image_generation"),
            ("browserUse", "浏览器操作", "browser_use"),
            ("computerUse", "电脑操作", "computer_use"),
        ] {
            let native_key = format!("features.{key}");
            let binding = match key {
                "image_generation" => "features.image_generation",
                "browser_use" => "features.browser_use",
                _ => "features.computer_use",
            };
            let mut field = BoundSetting::new(
                id,
                "tools",
                label,
                &format!("-c {native_key}"),
                Binding::Codex(binding, ValueType::Boolean),
            )
            .choices(&boolean())
            .describe(
                "控制原生功能开关。实际可用性仍取决于当前 Codex 版本、模型、账号及已安装组件。",
            )
            .current(
                snapshot
                    .get(&format!("/features/{key}"))
                    .or_else(|| snapshot.feature_default(key)),
                "未设置",
            )
            .unset_detail("未设置", &snapshot.feature_defaults_notice);
            if snapshot.requirement(&format!("/features/{key}")).is_some() {
                field = field.locked("此功能由 Codex 管理策略固定");
            }
            fields.push(field);
        }
        fields.push(
            BoundSetting::new(
                "planTool",
                "tools",
                "计划维护工具",
                "-c tools.update_plan.enabled",
                Binding::Codex("tools.update_plan.enabled", ValueType::Boolean),
            )
            .choices(&boolean())
            .current(snapshot.get("/tools/update_plan/enabled"), "未设置")
            .unset_detail(
                "未设置",
                "未设置 tools.update_plan.enabled，当前 CLI 没有提供可确认的默认启用状态。",
            ),
        );
    }
    fields
}
