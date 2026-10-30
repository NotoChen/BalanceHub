use super::{
    assets::{self, LaunchAssets},
    PreparedRunSettings,
};
use crate::services::agent_cli::settings::json_config;
use crate::{
    models::*,
    services::agent_cli::{
        self,
        contracts::{AgentFilePlan, TemporaryLaunchPlan},
    },
};
use serde_json::{json, Value};
use std::path::Path;

pub(super) fn apply(
    kind: AgentCliKind,
    settings: &TemporaryCliRunSettings,
    assets: &LaunchAssets,
    mode: TemporaryCliSessionMode,
    directory: &Path,
    plan: &mut TemporaryLaunchPlan,
    prepared: &mut PreparedRunSettings,
) -> Result<(), String> {
    match kind {
        AgentCliKind::Codex => codex(settings, assets, plan)?,
        AgentCliKind::ClaudeCode => claude(settings, assets, mode, directory, plan, prepared)?,
        AgentCliKind::Gemini => gemini(settings, assets, plan)?,
        AgentCliKind::Grok => {
            if mode == TemporaryCliSessionMode::New {
                pair(&mut plan.args, "--rules", &settings.instructions);
            }
        }
    }
    for directory in &settings.additional_directories {
        pair(
            &mut plan.args,
            if kind == AgentCliKind::Gemini {
                "--include-directories"
            } else {
                "--add-dir"
            },
            directory,
        );
    }
    Ok(())
}

pub(super) fn preserve_gemini_settings(
    workdir: &Path,
    plan: &mut TemporaryLaunchPlan,
) -> Result<(), String> {
    if plan.settings_document().is_some() {
        let value = agent_cli::merge_gemini_temporary_settings(workdir, json_config(plan)?)?;
        plan.write_settings_document(
            serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?,
        )?;
    }
    Ok(())
}

pub(super) fn append_prompt(
    kind: AgentCliKind,
    settings: &TemporaryCliRunSettings,
    mode: TemporaryCliSessionMode,
    plan: &mut TemporaryLaunchPlan,
) {
    let prompt = if (mode == TemporaryCliSessionMode::History
        || (kind != AgentCliKind::ClaudeCode && kind != AgentCliKind::Grok))
        && !settings.instructions.trim().is_empty()
    {
        format!(
            "本次工作要求：\n{}\n\n{}",
            settings.instructions.trim(),
            settings.initial_prompt.trim()
        )
    } else {
        settings.initial_prompt.trim().to_string()
    };
    if !prompt.is_empty() {
        if kind == AgentCliKind::Gemini {
            pair(&mut plan.args, "--prompt-interactive", &prompt);
        } else {
            plan.args.extend(["--".into(), prompt]);
        }
    }
}

fn codex(
    settings: &TemporaryCliRunSettings,
    assets: &LaunchAssets,
    plan: &mut TemporaryLaunchPlan,
) -> Result<(), String> {
    if !settings.keep_session {
        plan.args.push("--no-daemon".into());
        config(plan, "history.persistence", "\"none\"");
        let home = plan
            .environment
            .set_values()
            .find(|(key, _)| *key == "CODEX_HOME")
            .map(|(_, value)| value.to_owned());
        if let Some(home) = home {
            plan.environment.set("CODEX_SQLITE_HOME", &home);
            config(plan, "sqlite_home", &toml_string(&home));
            config(
                plan,
                "log_dir",
                &toml_string(&Path::new(&home).join("log").to_string_lossy()),
            );
        }
    }
    if !settings.load_rules {
        config(plan, "project_doc_max_bytes", "0");
    }
    if settings.disable_hooks {
        config(plan, "features.hooks", "false");
    }
    for (requested, category) in [
        (settings.disable_skills, AgentAssetCategory::Skill),
        (settings.disable_mcp, AgentAssetCategory::Mcp),
        (settings.disable_plugins, AgentAssetCategory::Plugin),
    ] {
        if requested {
            assets.require_complete(category)?;
        }
    }
    let selected = assets
        .items
        .iter()
        .filter(|asset| assets::requested(settings, asset))
        .collect::<Vec<_>>();
    for asset in &selected {
        match asset.option.category {
            AgentAssetCategory::Mcp => {
                if let Some(parent) = &asset.parent {
                    let parent_id =
                        assets::asset_id(AgentCliKind::Codex, parent.category, &parent.native_id);
                    if asset.enabled
                        && !settings.disable_plugins
                        && !settings.disabled_assets.contains(&parent_id)
                    {
                        return Err(format!(
                            "{} 由插件 {} 提供；要停用全部 MCP，请同时停用该插件",
                            asset.option.label, parent.native_id
                        ));
                    }
                } else {
                    config(
                        plan,
                        &format!("mcp_servers.{}.enabled", toml_string(&asset.native_id)),
                        "false",
                    );
                }
            }
            AgentAssetCategory::Plugin => config(
                plan,
                &format!("plugins.{}.enabled", toml_string(&asset.native_id)),
                "false",
            ),
            _ => {}
        }
    }
    let skills = selected
        .iter()
        .filter(|asset| asset.option.category == AgentAssetCategory::Skill)
        .collect::<Vec<_>>();
    if !skills.is_empty() {
        let mut rules = Vec::new();
        for source in assets.configs("config.toml")? {
            if let Some(value) = source.pointer("/skills/config") {
                rules = value
                    .as_array()
                    .ok_or("原有 skills.config 格式无效")?
                    .clone();
            }
        }
        for asset in skills {
            for rule in &mut rules {
                let matches_name =
                    rule.get("name").and_then(Value::as_str) == Some(asset.native_id.as_str());
                let matches_path = rule
                    .get("path")
                    .and_then(Value::as_str)
                    .is_some_and(|path| asset.paths.iter().any(|p| p == Path::new(path)));
                if matches_name || matches_path {
                    rule["enabled"] = json!(false);
                }
            }
            rules.push(json!({"name":asset.native_id,"enabled":false}));
        }
        let value =
            toml::Value::try_from(&rules).map_err(|e| format!("生成本次 Skill 规则失败：{e}"))?;
        config(plan, "skills.config", &value.to_string());
    }
    if settings.disable_skills {
        config(plan, "skills.bundled.enabled", "false");
        config(plan, "skills.include_instructions", "false");
    }
    Ok(())
}

fn claude(
    settings: &TemporaryCliRunSettings,
    assets: &LaunchAssets,
    mode: TemporaryCliSessionMode,
    directory: &Path,
    plan: &mut TemporaryLaunchPlan,
    prepared: &mut PreparedRunSettings,
) -> Result<(), String> {
    if mode == TemporaryCliSessionMode::New {
        pair(
            &mut plan.args,
            "--append-system-prompt",
            &settings.instructions,
        );
    }
    let mut config = json_config(plan)?;
    if settings.disable_hooks {
        config["disableAllHooks"] = json!(true);
    }
    if settings.disable_skills {
        plan.args.push("--disable-slash-commands".into());
    }
    if settings.disable_plugins {
        assets.require_complete(AgentAssetCategory::Plugin)?;
    }
    let mut denied = Vec::new();
    let mut denied_mcp = Vec::new();
    for asset in assets
        .items
        .iter()
        .filter(|asset| assets::requested(settings, asset))
    {
        match asset.option.category {
            AgentAssetCategory::Skill if !settings.disable_skills => {
                denied.push(format!("Skill({})", asset.native_id))
            }
            AgentAssetCategory::Mcp if !settings.disable_mcp => {
                denied_mcp.push(json!({"serverName":asset.native_id}))
            }
            AgentAssetCategory::Plugin => {
                config["enabledPlugins"][&asset.native_id] = json!(false);
            }
            _ => {}
        }
    }
    if !denied.is_empty() {
        config["permissions"]["deny"] = json!(denied);
    }
    // Claude merges the denylist across settings scopes. Adding native server
    // names keeps plugin namespaces, authentication and existing policies intact.
    if !denied_mcp.is_empty() {
        config["deniedMcpServers"] = json!(denied_mcp);
    }
    if settings.disable_mcp {
        if assets.has_managed_mcp() {
            return Err("Claude 的 MCP 由组织配置统一提供，原生 CLI 不允许整组替换；可保留 MCP 后选择其他本次设置".into());
        }
        let path = directory.join("mcp.json");
        prepared.files.push(AgentFilePlan {
            path: path.clone(),
            content: "{\"mcpServers\":{}}".into(),
        });
        plan.args.push("--strict-mcp-config".into());
        pair(&mut plan.args, "--mcp-config", &path.to_string_lossy());
    }
    plan.write_settings_document(
        serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?,
    )?;
    Ok(())
}

fn gemini(
    settings: &TemporaryCliRunSettings,
    assets: &LaunchAssets,
    plan: &mut TemporaryLaunchPlan,
) -> Result<(), String> {
    let mut config = json_config(plan)?;
    let selected = |category| {
        assets
            .items
            .iter()
            .filter(|asset| asset.option.category == category && assets::requested(settings, asset))
            .map(|asset| asset.native_id.clone())
            .collect::<Vec<_>>()
    };
    if settings.disable_skills {
        config["skills"]["enabled"] = json!(false);
    } else {
        let mut skills = selected(AgentAssetCategory::Skill);
        if !skills.is_empty() {
            append_native_disabled(assets, "/skills/disabled", &mut skills)?;
            config["skills"]["disabled"] = json!(skills);
        }
    }
    if settings.disable_mcp {
        config["admin"]["mcp"]["enabled"] = json!(false);
    } else {
        let mut mcp = selected(AgentAssetCategory::Mcp);
        if !mcp.is_empty() {
            append_native_disabled(assets, "/mcp/excluded", &mut mcp)?;
            config["mcp"]["excluded"] = json!(mcp);
        }
    }
    if settings.disable_hooks {
        config["hooksConfig"]["enabled"] = json!(false);
    }
    if settings.disable_plugins {
        pair(&mut plan.args, "--extensions", "none");
    } else if !selected(AgentAssetCategory::Extension).is_empty() {
        assets.require_complete(AgentAssetCategory::Extension)?;
        let extensions: Vec<_> = assets
            .items
            .iter()
            .filter(|asset| {
                asset.option.category == AgentAssetCategory::Extension
                    && asset.enabled
                    && !assets::requested(settings, asset)
            })
            .collect();
        if extensions.is_empty() {
            pair(&mut plan.args, "--extensions", "none");
        } else {
            for extension in extensions {
                pair(&mut plan.args, "--extensions", &extension.native_id);
            }
        }
    }
    plan.write_settings_document(
        serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?,
    )?;
    Ok(())
}

fn append_native_disabled(
    assets: &LaunchAssets,
    key: &str,
    names: &mut Vec<String>,
) -> Result<(), String> {
    for source in assets
        .configs("settings.json")?
        .into_iter()
        .chain(assets.configs("system-defaults.json")?)
    {
        if let Some(value) = source.pointer(key) {
            let original = value
                .as_array()
                .ok_or_else(|| format!("原生配置 {key} 必须是列表"))?;
            for name in original {
                let name = name
                    .as_str()
                    .ok_or_else(|| format!("原生配置 {key} 包含无效项"))?;
                if !names.iter().any(|value| value == name) {
                    names.push(name.into());
                }
            }
        }
    }
    Ok(())
}

fn pair(args: &mut Vec<String>, flag: &str, value: &str) {
    if !value.trim().is_empty() {
        args.extend([flag.to_string(), value.trim().to_string()]);
    }
}
fn config(plan: &mut TemporaryLaunchPlan, key: &str, value: &str) {
    plan.args.extend(["-c".into(), format!("{key}={value}")]);
}
fn toml_string(value: &str) -> String {
    toml::Value::String(value.into()).to_string()
}
