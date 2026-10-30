use crate::services::agent_cli::settings as options;
mod assets;
mod capabilities;
mod native;
mod retention;

use crate::{
    models::*,
    services::agent_cli::contracts::{AgentFilePlan, TemporaryLaunchPlan},
};
use std::{fs, path::Path};

pub(crate) use capabilities::capabilities;

pub(super) struct PreparedRunSettings {
    pub(super) files: Vec<AgentFilePlan>,
    isolation: Option<retention::IsolatedHome>,
    pub(super) summary: Vec<String>,
    pub(super) model_selection: AgentSettingValueSummary,
}

impl PreparedRunSettings {
    pub(super) fn materialize(&self) -> Result<(), String> {
        if let Some(isolation) = &self.isolation {
            isolation.materialize()?;
        }
        for file in &self.files {
            fs::write(&file.path, &file.content)
                .map_err(|e| format!("写入本次启动配置失败：{e}"))?;
            super::shell_runtime::script::restrict_to_owner(&file.path)?;
        }
        Ok(())
    }
}

pub(crate) fn validate(settings: &TemporaryCliRunSettings) -> Result<(), String> {
    if settings.initial_prompt.len() + settings.instructions.len() > 256 * 1024 {
        return Err("本次上下文超过 256 KiB，请改为在会话中按需读取文件".into());
    }
    if settings.disabled_assets.len() > 4096 || settings.additional_directories.len() > 64 {
        return Err("本次启动选项数量过多".into());
    }
    if settings.initial_prompt.contains('\0') || settings.instructions.contains('\0') {
        return Err("上下文不能包含空字符".into());
    }
    Ok(())
}

pub(crate) fn asset_options(
    settings: &AppSettings,
    request: &TemporaryCliSettingsRequest,
    refresh: bool,
) -> Result<TemporaryCliAssetListing, String> {
    let workdir = crate::services::workspaces::normalize_directory(Path::new(&request.workdir))?;
    let mut settings = settings.clone();
    settings.set_agent_cli_path(request.cli_kind, request.cli_path.clone());
    Ok(assets::load(&settings, request.cli_kind, &workdir, refresh)?.listing())
}

pub(super) struct RunSettingsRequest<'a> {
    pub app_settings: &'a AppSettings,
    pub kind: AgentCliKind,
    pub cli_path: &'a str,
    pub workdir: &'a Path,
    pub mode: TemporaryCliSessionMode,
    pub model: &'a str,
    pub inherited_model: &'a str,
    pub directory: &'a Path,
    pub settings: Option<&'a TemporaryCliRunSettings>,
}

pub(super) fn apply(
    request: RunSettingsRequest<'_>,
    plan: &mut TemporaryLaunchPlan,
) -> Result<PreparedRunSettings, String> {
    let RunSettingsRequest {
        app_settings,
        kind,
        cli_path,
        workdir,
        mode,
        model,
        inherited_model,
        directory,
        settings,
    } = request;
    let default = TemporaryCliRunSettings::default();
    let settings = settings.unwrap_or(&default);
    let model_request = TemporaryCliSettingsRequest {
        native_options: settings.native_options.clone(),
        cli_kind: kind,
        cli_path: cli_path.into(),
        workdir: workdir.to_string_lossy().into_owned(),
        model: model.into(),
        inherited_model: inherited_model.into(),
        session_mode: mode,
    };
    if settings == &default {
        if kind == AgentCliKind::Gemini {
            native::preserve_gemini_settings(workdir, plan)?;
        }
        return Ok(PreparedRunSettings {
            files: Vec::new(),
            isolation: None,
            summary: vec!["保存会话".into(), "未覆盖其他运行参数".into()],
            model_selection: options::read_model_selection(&model_request),
        });
    }
    validate(settings)?;
    let (capabilities, resolved) = capabilities::inspect(&model_request)?;
    let mut prepared = PreparedRunSettings {
        files: Vec::new(),
        isolation: None,
        summary: Vec::new(),
        model_selection: capabilities.model.clone(),
    };
    options::validate_options(settings, &resolved)?;
    for (requested, supported, label) in [
        (
            !settings.keep_session,
            capabilities.can_discard_session,
            "不保留会话",
        ),
        (
            !settings.load_rules,
            capabilities.can_skip_rules,
            "跳过工作规则",
        ),
        (
            !settings.additional_directories.is_empty(),
            capabilities.can_add_directories,
            "附加目录",
        ),
        (
            settings.disable_skills,
            capabilities.can_disable_skills,
            "停用 Skill",
        ),
        (
            settings.disable_mcp,
            capabilities.can_disable_mcp,
            "停用 MCP",
        ),
        (
            settings.disable_plugins,
            capabilities.can_disable_plugins,
            "停用插件",
        ),
        (
            settings.disable_hooks,
            capabilities.can_disable_hooks,
            "停用 Hook",
        ),
    ] {
        if requested && !supported {
            return Err(format!("当前 Agent 版本不支持本次启动选项：{label}"));
        }
    }
    if mode == TemporaryCliSessionMode::History && !settings.keep_session {
        return Err("继续已有会话会保留会话记录；如需一次性会话，请新建会话".into());
    }
    for path in &settings.additional_directories {
        crate::services::workspaces::normalize_directory(Path::new(path))?;
    }
    let needs_assets = !settings.disabled_assets.is_empty()
        || (kind == AgentCliKind::Codex
            && (settings.disable_skills || settings.disable_mcp || settings.disable_plugins))
        || (kind == AgentCliKind::ClaudeCode && (settings.disable_mcp || settings.disable_plugins));
    let assets = if needs_assets {
        let mut app_settings = app_settings.clone();
        app_settings.set_agent_cli_path(kind, cli_path.into());
        assets::load(&app_settings, kind, workdir, false)?
    } else {
        assets::LaunchAssets::default()
    };
    for id in &settings.disabled_assets {
        let asset = assets
            .items
            .iter()
            .find(|asset| &asset.option.id == id)
            .ok_or("启动方案中的资产不在当前项目环境中，请刷新资产并调整方案")?;
        if !asset.option.configurable {
            return Err(format!("{}：{}", asset.option.label, asset.option.note));
        }
    }
    if !settings.keep_session {
        let isolated_home = plan.isolated_home();
        let seeded = plan
            .auxiliary_files
            .iter()
            .map(|file| file.path.to_string())
            .collect::<Vec<_>>();
        prepared.isolation = Some(retention::IsolatedHome::plan(
            kind,
            directory,
            &mut plan.environment,
            isolated_home,
            &seeded,
        )?);
    }
    native::apply(
        kind,
        settings,
        &assets,
        mode,
        directory,
        plan,
        &mut prepared,
    )?;
    options::apply(settings, &resolved, plan)?;
    if kind == AgentCliKind::Gemini {
        native::preserve_gemini_settings(workdir, plan)?;
    }
    native::append_prompt(kind, settings, mode, plan);
    validate_argument_size(cli_path, &plan.args)?;
    prepared.summary = summary(settings, &capabilities);
    Ok(prepared)
}

fn validate_argument_size(cli_path: &str, args: &[String]) -> Result<(), String> {
    let values = std::iter::once(cli_path).chain(args.iter().map(String::as_str));
    #[cfg(windows)]
    let too_long = values
        .map(|value| value.encode_utf16().count() * 2 + 4)
        .sum::<usize>()
        > 28_000;
    #[cfg(not(windows))]
    let too_long = values.map(|value| value.len() + 1).sum::<usize>() > 128 * 1024;
    if too_long {
        return Err(
            "本次上下文和资产选项超过系统的启动参数长度，请精简后重试；较长资料可在会话中读取文件"
                .into(),
        );
    }
    Ok(())
}

fn summary(
    settings: &TemporaryCliRunSettings,
    capabilities: &TemporaryCliSettingsCapabilities,
) -> Vec<String> {
    let mut result = vec![if settings.keep_session {
        "保存会话"
    } else {
        "退出后清除本次会话"
    }
    .into()];
    for field in &capabilities.native_settings {
        if let Some(value) = settings.native_options.get(&field.id) {
            let display = if field.control == AgentSettingControl::ModelMappings {
                serde_json::from_str::<std::collections::BTreeMap<String, AgentModelMappingValue>>(
                    value,
                )
                .map(|mappings| {
                    mappings
                        .into_iter()
                        .map(|(alias, mapping)| {
                            if let Some(model) = mapping.model {
                                format!("{alias} → {model}")
                            } else if let Some(parent) = mapping.extends {
                                format!("{alias} 继承 {parent}")
                            } else {
                                format!("{alias}：{}", mapping.display_name.unwrap_or_default())
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("；")
                })
                .unwrap_or_else(|_| "模型映射".into())
            } else if matches!(
                field.control,
                AgentSettingControl::MultiSelect | AgentSettingControl::Rules
            ) {
                format!(
                    "{} 项",
                    value.lines().filter(|line| !line.trim().is_empty()).count()
                )
            } else {
                field
                    .choices
                    .iter()
                    .find(|choice| &choice.value == value)
                    .map(|choice| choice.label.clone())
                    .unwrap_or_else(|| value.clone())
            };
            result.push(format!(
                "{}：{}{}",
                field.label,
                display,
                if field.unit.is_empty() {
                    String::new()
                } else {
                    format!(" {}", field.unit)
                }
            ));
        }
    }
    for (enabled, label) in [
        (settings.disable_skills, "停用 Skill"),
        (settings.disable_mcp, "停用 MCP"),
        (settings.disable_plugins, "停用插件"),
        (settings.disable_hooks, "停用 Hook"),
        (!settings.load_rules, "跳过工作规则"),
    ] {
        if enabled {
            result.push(label.into());
        }
    }
    if !settings.disabled_assets.is_empty() {
        result.push(format!("停用 {} 项资产", settings.disabled_assets.len()));
    }
    if !settings.instructions.is_empty() || !settings.initial_prompt.is_empty() {
        result.push("包含本次任务与工作要求".into());
    }
    if !settings.additional_directories.is_empty() {
        result.push(format!(
            "附加 {} 个目录",
            settings.additional_directories.len()
        ));
    }
    result
}
