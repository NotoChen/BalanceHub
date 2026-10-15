use super::*;

pub(super) enum Storage {
    Key(String, ValueType),
    ClaudeModel,
    Sandbox,
    BooleanFlag(String),
    Approval,
    ApprovalCategories,
    Subagents,
    SubagentLimit,
    GeminiModel(&'static str),
    ModelMappings(AgentCliKind),
}

const CATEGORIES: [&str; 5] = [
    "sandbox_approval",
    "rules",
    "mcp_elicitations",
    "request_permissions",
    "skill_approval",
];

impl Storage {
    pub(super) fn read(&self, root: &Value, model: &str) -> Option<String> {
        match self {
            Self::ClaudeModel => root
                .pointer("/env/ANTHROPIC_MODEL")
                .or_else(|| root.get("model"))
                .map(sources::display_value),
            Self::ModelMappings(kind) => model_mappings::read(*kind, root)
                .ok()
                .filter(|values| !values.is_empty())
                .and_then(|values| serde_json::to_string(&values).ok()),
            Self::Key(pointer, ValueType::Percent) => root
                .pointer(pointer)?
                .as_f64()
                .map(|value| (value * 100.0).round().to_string()),
            Self::Key(pointer, _) => root.pointer(pointer).map(sources::display_value),
            Self::SubagentLimit => codex::v2_child_count(root.pointer(codex::V2_LIMIT_POINTER)?)
                .map(|value| value.to_string()),
            Self::Sandbox => root.pointer("/tools/sandbox").map(sources::display_value),
            Self::BooleanFlag(pointer) => root
                .pointer(pointer)?
                .as_bool()
                .map(|value| if value { "1" } else { "0" }.into()),
            Self::Approval => root.get("approval_policy").map(|value| {
                if value.get("granular").is_some() {
                    "granular".into()
                } else {
                    sources::display_value(value)
                }
            }),
            Self::ApprovalCategories => root
                .pointer("/approval_policy/granular")
                .and_then(Value::as_object)
                .map(|items| {
                    CATEGORIES
                        .iter()
                        .filter(|key| items.get(**key) != Some(&json!(false)))
                        .copied()
                        .collect::<Vec<_>>()
                        .join("\n")
                }),
            Self::Subagents => root
                .pointer("/features/multi_agent_v2")
                .filter(|value| {
                    value
                        .as_bool()
                        .or_else(|| value.get("enabled").and_then(Value::as_bool))
                        == Some(true)
                })
                .map(|_| "true".into())
                .or_else(|| {
                    root.pointer("/agents/enabled")
                        .or_else(|| root.pointer("/features/multi_agent"))
                        .or_else(|| root.pointer("/features/multi_agent_v2/enabled"))
                        .or_else(|| {
                            root.pointer("/features/multi_agent_v2")
                                .filter(|value| value.is_boolean())
                        })
                        .map(sources::display_value)
                }),
            Self::GeminiModel(key) => gemini_model::read(root, model, key),
        }
    }

    pub(super) fn write(
        &self,
        root: &mut Value,
        model: &str,
        value: Option<&str>,
        snapshot: &NativeSnapshot,
    ) -> Result<(), String> {
        match self {
            Self::ClaudeModel => {
                document::set(root, "/model", value.map(|value| json!(value)))?;
                document::set(root, "/env/ANTHROPIC_MODEL", None)
            }
            Self::ModelMappings(kind) => model_mappings::write(*kind, root, value, snapshot),
            Self::Key(pointer, kind) => document::set(
                root,
                pointer,
                value.map(|value| typed(value, *kind)).transpose()?,
            ),
            Self::SubagentLimit => document::set(
                root,
                codex::V2_LIMIT_POINTER,
                value.map(codex::v2_total_count).transpose()?,
            ),
            Self::Sandbox => document::set(
                root,
                "/tools/sandbox",
                value.map(|value| {
                    value
                        .parse::<bool>()
                        .map(Value::Bool)
                        .unwrap_or_else(|_| json!(value))
                }),
            ),
            Self::BooleanFlag(pointer) => {
                document::set(root, pointer, value.map(|value| json!(value == "1")))
            }
            Self::Approval => {
                let policy = value.map(|value| if value == "granular" {
                    root.get("approval_policy").filter(|value| value.get("granular").is_some()).cloned()
                        .unwrap_or_else(|| json!({"granular": CATEGORIES.into_iter().map(|key| (key, true)).collect::<BTreeMap<_, _>>()}))
                } else { json!(value) });
                document::set(root, "/approval_policy", policy)
            }
            Self::ApprovalCategories => {
                let Some(value) = value else {
                    // Granular approval is one native enum variant. Removing
                    // its category selection must also remove the wrapper.
                    return document::set(root, "/approval_policy", None);
                };
                if root.pointer("/approval_policy/granular").is_none() {
                    return Err("先选择按类别审批，再设置审批类别".into());
                }
                let selected = entries(value);
                for category in CATEGORIES {
                    document::set(
                        root,
                        &format!("/approval_policy/granular/{category}"),
                        Some(json!(selected.iter().any(|item| item == category))),
                    )?;
                }
                Ok(())
            }
            Self::Subagents => {
                let value = value
                    .map(|value| typed(value, ValueType::Boolean))
                    .transpose()?;
                let has_v2 = root.pointer("/features/multi_agent_v2").is_some();
                let v2_key = if root
                    .pointer("/features/multi_agent_v2")
                    .is_some_and(Value::is_object)
                {
                    "/features/multi_agent_v2/enabled"
                } else {
                    "/features/multi_agent_v2"
                };
                document::set(root, "/agents/enabled", value.clone())?;
                document::set(root, "/features/multi_agent", value.clone())?;
                if has_v2 && (value.is_none() || value == Some(json!(false))) {
                    document::set(root, v2_key, value)?;
                }
                Ok(())
            }
            Self::GeminiModel(key) => gemini_model::write(root, model, key, value, snapshot),
        }
    }
}

pub(super) fn storage(
    field: &mut BoundSetting,
    source: &ConfigurationSourceAuthority,
    model: &str,
) -> Option<Storage> {
    let kind = source.source.agent_kind;
    let store = if field.ui.id == "model" {
        field.ui.control = AgentSettingControl::Model;
        if kind == AgentCliKind::ClaudeCode {
            field.ui.description.push_str(" 保存到 model，并移除本文件中优先级更高的 ANTHROPIC_MODEL，避免覆盖 /model 的后续选择；外部环境变量仍按原生规则生效。");
            Storage::ClaudeModel
        } else {
            Storage::Key(model_pointer(kind).into(), ValueType::Text)
        }
    } else {
        match &field.binding {
            Binding::Codex(key, value_type) => {
                Storage::Key(format!("/{}", key.replace('.', "/")), *value_type)
            }
            Binding::CodexApproval => Storage::Approval,
            Binding::CodexGranular => Storage::ApprovalCategories,
            Binding::CodexSubagents => Storage::Subagents,
            Binding::CodexSubagentLimit => Storage::SubagentLimit,
            Binding::Json(pointer, value_type) => Storage::Key((*pointer).into(), *value_type),
            Binding::JsonAppend(pointer) => {
                field.ui.description = "每行一条规则，编辑的是当前文件中的完整列表；其他来源的规则仍按 Agent 的原生优先级合并。".into();
                Storage::Key((*pointer).into(), ValueType::List)
            }
            Binding::GeminiModel(key) => Storage::GeminiModel(key),
            Binding::ModelMappings => Storage::ModelMappings(kind),
            Binding::Env(name) if kind == AgentCliKind::ClaudeCode => {
                Storage::Key(format!("/env/{name}"), ValueType::Text)
            }
            Binding::Env("GEMINI_SANDBOX") => Storage::Sandbox,
            Binding::Env("GROK_LSP_TOOLS") => Storage::BooleanFlag("/features/lsp_tools".into()),
            Binding::Flag(flag) => {
                let key = match (kind, *flag) {
                    (AgentCliKind::Codex, "--sandbox") => "/sandbox_mode",
                    (AgentCliKind::ClaudeCode, "--permission-mode") => "/permissions/defaultMode",
                    (AgentCliKind::Gemini, "--approval-mode") => "/general/defaultApprovalMode",
                    (AgentCliKind::Grok, "--reasoning-effort") => {
                        "/models/default_reasoning_effort"
                    }
                    (AgentCliKind::Grok, "--permission-mode") => "/ui/permission_mode",
                    (AgentCliKind::Grok, "--sandbox") => "/sandbox/profile",
                    _ => return None,
                };
                let key = if kind == AgentCliKind::Grok
                    && *flag == "--reasoning-effort"
                    && !model.is_empty()
                {
                    format!("/model/{}/reasoning_effort", native_model_pointer(model))
                } else {
                    key.into()
                };
                Storage::Key(key, ValueType::Text)
            }
            Binding::FlagRules(flag) => {
                let key = match (kind, *flag) {
                    (AgentCliKind::ClaudeCode, "--allowed-tools") => "/permissions/allow",
                    (AgentCliKind::ClaudeCode, "--disallowed-tools") => "/permissions/deny",
                    (AgentCliKind::Grok, "--allow") => "/permission/allow",
                    (AgentCliKind::Grok, "--deny") => "/permission/deny",
                    _ => return None,
                };
                field.ui.description =
                    "每行一条原生工具规则。只修改当前文件的规则列表，其他来源的规则保持原生行为。"
                        .into();
                Storage::Key(key.into(), ValueType::List)
            }
            Binding::Readonly if kind == AgentCliKind::Grok => {
                let (pointer, maximum, unit) = match field.ui.id.as_str() {
                    "contextWindow" if !model.is_empty() => (
                        format!("/model/{}/context_window", native_model_pointer(model)),
                        None,
                        "tokens",
                    ),
                    "compressionThreshold" => (
                        "/session/auto_compact_threshold_percent".into(),
                        Some(100),
                        "%",
                    ),
                    "outputLimit" => ("/models/max_completion_tokens".into(), None, "tokens"),
                    _ => return None,
                };
                field.ui.control = AgentSettingControl::Number;
                field.ui.minimum = Some(1);
                field.ui.maximum = maximum;
                field.ui.unit = unit.into();
                field.ui.description =
                    "保存到 Grok 的原生配置；数值仍受模型和服务端容量约束。".into();
                Storage::Key(pointer, ValueType::Integer)
            }
            _ => return None,
        }
    };
    // Grok deliberately ignores model/UI/tool defaults in project config.
    if kind == AgentCliKind::Grok
        && source.source.native_role == "workspace-config"
        && !matches!(&store, Storage::Key(pointer, _) if pointer.starts_with("/permission/"))
    {
        return None;
    }
    field.ui.native_key = match &store {
        Storage::ClaudeModel => "model / env.ANTHROPIC_MODEL".into(),
        Storage::Key(pointer, _) | Storage::BooleanFlag(pointer) => pointer
            .trim_start_matches('/')
            .split('/')
            .map(|part| part.replace("~1", "/").replace("~0", "~"))
            .collect::<Vec<_>>()
            .join("."),
        Storage::Sandbox => "tools.sandbox".into(),
        Storage::Approval => "approval_policy".into(),
        Storage::ApprovalCategories => "approval_policy.granular".into(),
        Storage::Subagents => "agents.enabled / features.multi_agent".into(),
        Storage::SubagentLimit => codex::V2_LIMIT_KEY.into(),
        Storage::GeminiModel(key) => format!("modelConfigs.customOverrides → {model} → {key}"),
        Storage::ModelMappings(_) => field.ui.native_key.clone(),
    };
    field.ui.label = field.ui.label.replace("本次可用", "可用");
    field.ui.description = field
        .ui
        .description
        .replace(
            "本次通过环境变量设置，避免被已有环境变量盖过。",
            "变量保存在当前文件的 env 中；已有启动环境可能按原生优先级覆盖它。",
        )
        .replace("本次", "此配置");
    if kind == AgentCliKind::ClaudeCode
        && field.ui.id == "approval"
        && matches!(
            source.source.scope,
            AgentAssetScope::Workspace | AgentAssetScope::Local
        )
    {
        field
            .ui
            .choices
            .retain(|choice| !matches!(choice.value.as_str(), "auto" | "bypassPermissions"));
    }
    Some(store)
}

pub(super) fn additional_fields(snapshot: &NativeSnapshot, fields: &mut Vec<BoundSetting>) {
    let directories = match snapshot.kind {
        AgentCliKind::Codex => Some((
            "/sandbox_workspace_write/writable_roots",
            "额外可写目录",
            "项目内可写时，允许修改其他仓库或共享目录；完全访问时无需添加。",
        )),
        AgentCliKind::ClaudeCode => Some((
            "/permissions/additionalDirectories",
            "额外工作目录",
            "允许工具访问另一个仓库或共享目录，操作仍遵循权限规则。",
        )),
        AgentCliKind::Gemini => Some((
            "/context/includeDirectories",
            "额外工作目录",
            "把其他仓库或共享目录加入工作范围，便于跨目录读取和协作。",
        )),
        AgentCliKind::Grok => None,
    };
    if let Some((pointer, label, hint)) = directories {
        let mut directories = BoundSetting::new(
                "additionalDirectories",
                "context",
                label,
                pointer,
                Binding::Json(pointer, ValueType::List),
            )
            .rules()
            .describe(
                "每行一个目录，建议使用绝对路径。按当前 Agent 的原生路径与沙盒规则解析；不会把整个目录的内容自动塞进上下文。",
            )
            .hint(hint)
            .current(snapshot.get(pointer), "未添加额外目录").unset("未添加额外目录");
        if snapshot.kind == AgentCliKind::Codex {
            directories = directories.when("sandbox", "workspace-write");
        }
        fields.push(directories);
    }
    let toggles: &[(&str, &str, &str)] = match snapshot.kind {
        AgentCliKind::Codex => &[
            ("bundledSkills", "内置 Skill", "/skills/bundled/enabled"),
            ("hooks", "Hook", "/features/hooks"),
        ],
        AgentCliKind::ClaudeCode => &[("disableHooks", "停用全部 Hook", "/disableAllHooks")],
        AgentCliKind::Gemini => &[
            ("skills", "Skill", "/skills/enabled"),
            ("hooks", "Hook", "/hooksConfig/enabled"),
        ],
        AgentCliKind::Grok => &[],
    };
    for (id, label, pointer) in toggles {
        let mut field = BoundSetting::new(
            id,
            "tools",
            label,
            pointer,
            Binding::Json(pointer, ValueType::Boolean),
        )
        .choices(&boolean())
        .current(
            snapshot.get(pointer).or_else(|| {
                if snapshot.kind == AgentCliKind::Codex && *id == "hooks" {
                    snapshot.feature_default("hooks")
                } else {
                    None
                }
            }),
            "未设置",
        )
        .unset_detail(
            "未设置",
            "当前配置没有设置此开关，所选 CLI 也未提供可确认的固定默认值。",
        );
        if snapshot.kind == AgentCliKind::Gemini {
            field = field.default(json!(true), "Gemini 内置默认值");
        } else if snapshot.kind == AgentCliKind::ClaudeCode {
            field = field.default(json!(false), "Claude 内置默认值（允许执行已配置的 Hook）");
        }
        fields.push(field);
    }
    if snapshot.kind == AgentCliKind::Codex {
        fields.push(
            BoundSetting::new(
                "ruleBytes",
                "context",
                "规则文本上限",
                "project_doc_max_bytes",
                Binding::Codex("project_doc_max_bytes", ValueType::Integer),
            )
            .number(0, None, "字节")
            .describe("Codex 把 AGENTS.md 等规则文件合并到启动指令时，最多读取的文本字节数。超出部分不会进入启动规则。0 表示跳过；需要更长规则时可以增大此值。")
            .hint("默认 32 KiB（32,768 字节）；仅限制规则文本，与项目文件读取、历史会话检索无关。")
            .current(snapshot.get("/project_doc_max_bytes"), "32 KiB")
            .default(json!(32768), "Codex 内置默认值 · project_doc_max_bytes"),
        );
    }
}
