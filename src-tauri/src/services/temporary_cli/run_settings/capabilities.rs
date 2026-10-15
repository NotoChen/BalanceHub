use crate::{models::*, services::agent_cli::settings as options};
use std::collections::BTreeMap;

pub(crate) fn capabilities(
    request: &TemporaryCliSettingsRequest,
) -> Result<TemporaryCliSettingsCapabilities, String> {
    inspect(request).map(|(capabilities, _)| capabilities)
}

pub(super) fn inspect(
    request: &TemporaryCliSettingsRequest,
) -> Result<(TemporaryCliSettingsCapabilities, options::ResolvedOptions), String> {
    let help = options::read_help(&request.cli_path)?;
    let has = |flag: &str| help.contains(flag);
    let resolved = options::resolve(request, &help)?;
    let mut result = TemporaryCliSettingsCapabilities {
        can_discard_session: false,
        retention_note: String::new(),
        native_settings: resolved
            .fields
            .iter()
            .map(|field| field.ui.clone())
            .collect(),
        model: resolved.model_display.clone(),
        default_model: resolved.default_model.clone(),
        model_native_key: resolved.model_native_key.clone(),
        model_choices: resolved.model_choices.clone(),
        model_catalog: resolved.model_catalog.clone(),
        notices: resolved.snapshot.notices.clone(),
        asset_controls: Vec::new(),
        run_references: BTreeMap::new(),
        can_skip_rules: false,
        can_add_directories: false,
        can_disable_skills: false,
        can_disable_mcp: false,
        can_disable_plugins: false,
        can_disable_hooks: false,
    };
    match request.cli_kind {
        AgentCliKind::Codex => {
            result.can_discard_session = has("--no-daemon");
            result.can_skip_rules = has("--config");
            result.can_disable_skills = has("--config");
            result.can_disable_mcp = has("--config");
            result.can_disable_plugins = has("--config");
            result.can_disable_hooks = has("--config");
            result.can_add_directories = has("--add-dir");
        }
        AgentCliKind::ClaudeCode => {
            result.can_discard_session = has("--settings");
            result.can_disable_skills = has("--disable-slash-commands");
            result.can_disable_mcp = has("--strict-mcp-config");
            result.can_disable_plugins = has("--settings");
            result.can_disable_hooks = has("--settings");
            result.can_add_directories = has("--add-dir");
        }
        AgentCliKind::Gemini => {
            result.can_discard_session = true;
            result.can_disable_skills = true;
            result.can_disable_mcp = has("--allowed-mcp-server-names");
            result.can_disable_plugins = has("--extensions");
            result.can_disable_hooks = true;
            result.can_add_directories = has("--include-directories");
        }
        AgentCliKind::Grok => {
            result.retention_note =
                "Grok 的会话由常驻进程管理，当前使用原生会话记录，未提供可靠的临时丢弃入口".into();
        }
    }
    if !result.can_discard_session && result.retention_note.is_empty() {
        result.retention_note = "当前 CLI 版本未提供可确认的交互会话隔离能力".into();
    }
    let keys = match request.cli_kind {
        AgentCliKind::Codex => [
            "skills.config / skills.bundled.enabled",
            "mcp_servers.<name>.enabled",
            "plugins.<id>.enabled",
            "features.hooks",
        ],
        AgentCliKind::ClaudeCode => [
            "--disable-slash-commands / Skill(name)",
            "--strict-mcp-config / deniedMcpServers",
            "enabledPlugins",
            "disableAllHooks",
        ],
        AgentCliKind::Gemini => [
            "skills.enabled / skills.disabled",
            "admin.mcp.enabled / mcp.excluded",
            "--extensions",
            "hooksConfig.enabled",
        ],
        AgentCliKind::Grok => ["", "", "", ""],
    };
    for (index, (supported, key, label)) in [
        (result.can_disable_skills, "disableSkills", "Skill"),
        (result.can_disable_mcp, "disableMcp", "MCP"),
        (
            result.can_disable_plugins,
            "disablePlugins",
            if request.cli_kind == AgentCliKind::Gemini {
                "扩展"
            } else {
                "插件"
            },
        ),
        (result.can_disable_hooks, "disableHooks", "Hook"),
    ]
    .into_iter()
    .enumerate()
    {
        if supported {
            result.asset_controls.push(TemporaryCliAssetControl {
                key: key.into(),
                label: label.into(),
                native_key: keys[index].into(),
                description: "保持当前配置会保留已有启停状态与管理规则；本次停用只影响这次启动。"
                    .into(),
            });
        }
    }
    if request.session_mode == TemporaryCliSessionMode::History {
        result.notices.push("继续历史会话时，未覆盖的会话参数由 Agent 从原记录恢复；下方当前值展示的是所选目录的启动配置。".into());
    }
    let (retention, rules, prompt, instructions, directories) = match request.cli_kind {
        AgentCliKind::Codex => (
            "CODEX_HOME / CODEX_SQLITE_HOME / --no-daemon / history.persistence",
            "project_doc_max_bytes",
            "codex [PROMPT]",
            "随首条消息传入工作要求",
            "--add-dir",
        ),
        AgentCliKind::ClaudeCode => (
            "CLAUDE_CONFIG_DIR",
            "CLAUDE.md / 原生规则加载",
            "claude [prompt]",
            "--append-system-prompt",
            "--add-dir",
        ),
        AgentCliKind::Gemini => (
            "GEMINI_CLI_HOME",
            "GEMINI.md / context.fileName",
            "--prompt-interactive",
            "随 --prompt-interactive 传入工作要求",
            "--include-directories",
        ),
        AgentCliKind::Grok => (
            "Grok 原生会话记录",
            "AGENTS.md / GROK.md / 原生规则加载",
            "grok [PROMPT]",
            "--rules",
            "原生工作目录范围",
        ),
    };
    let instructions = if request.session_mode == TemporaryCliSessionMode::History {
        "随恢复后的首条消息传入工作要求"
    } else {
        instructions
    };
    for (key, native_key, description) in [
        ("keepSession", retention, "保存时保留 Agent 原生会话记录行为；不保存时使用独立的本次配置目录，退出后只清除本次会话数据，项目文件修改仍保留。"),
        ("loadRules", rules, "按 Agent 当前配置加载已有工作规则。Codex 的本次跳过使用 project_doc_max_bytes = 0；其他 Agent 未提供独立且可确认的跳过规则参数。"),
        ("initialPrompt", prompt, "作为启动或恢复会话后的第一条消息提交。"),
        ("instructions", instructions, "仅附加本次要求，不修改全局或项目规则文件；原生优先级由所示入口决定。"),
        ("additionalDirectories", directories, "追加本次可访问目录，仍受 Agent 的路径校验及沙盒约束。"),
    ] {
        result.run_references.insert(key.into(), AgentSettingReference { native_key: native_key.into(), description: description.into() });
    }
    Ok((result, resolved))
}
