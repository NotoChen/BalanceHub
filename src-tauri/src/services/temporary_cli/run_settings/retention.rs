use crate::{
    models::AgentCliKind,
    services::agent_cli::{self, contracts::EnvironmentPatch},
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub(super) struct IsolatedHome {
    source: PathBuf,
    target: PathBuf,
    kind: AgentCliKind,
}

impl IsolatedHome {
    pub(super) fn plan(
        kind: AgentCliKind,
        directory: &Path,
        environment: &mut EnvironmentPatch,
    ) -> Result<Self, String> {
        let adapter = agent_cli::definition(kind)
            .sessions()
            .ok_or("Agent 未提供原生会话目录")?
            .history();
        let source = (adapter.source)()?.config_root;
        let target = if kind == AgentCliKind::Gemini {
            directory.join("run-home/.gemini")
        } else {
            directory.join("run-home")
        };
        let patch = (agent_cli::definition(kind)
            .sessions()
            .ok_or("Agent 未提供独立配置目录")?
            .history()
            .launch_environment)(&target)
        .map_err(str::to_string)?;
        for name in patch.removed_names() {
            environment.remove(name);
        }
        for (name, value) in patch.set_values() {
            environment.set(name, value);
        }
        Ok(Self {
            source,
            target,
            kind,
        })
    }

    pub(super) fn materialize(&self) -> Result<(), String> {
        create_private_dir(&self.target)?;
        if !self.source.is_dir() {
            return Ok(());
        }
        let started = Instant::now();
        let mut budget = 128 * 1024 * 1024u64;
        for entry in
            fs::read_dir(&self.source).map_err(|e| format!("读取 Agent 配置目录失败：{e}"))?
        {
            check_deadline(started)?;
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry.file_name();
            let name_text = name.to_string_lossy();
            let path = entry.path();
            if path.is_dir() {
                if [
                    "skills",
                    "agents",
                    "commands",
                    "rules",
                    "hooks",
                    "plugins",
                    "extensions",
                ]
                .contains(&name_text.as_ref())
                {
                    mirror_assets(&path, &self.target.join(&name), &mut budget, started)?;
                }
            } else if matches!(
                path.extension().and_then(|s| s.to_str()),
                Some("toml" | "json" | "md")
            ) && ![
                "auth.json",
                "history.json",
                "history.jsonl",
                "oauth_creds.json",
                "google_accounts.json",
                "mcp-oauth-tokens-v2.json",
                ".credentials.json",
            ]
            .contains(&name_text.as_ref())
            {
                copy_file(&path, &self.target.join(&name), &mut budget)?;
                if self.kind == AgentCliKind::Codex && name_text == "config.toml" {
                    rebase_codex_paths(&self.source, &self.target.join(&name))?;
                }
            }
        }
        if self.kind == AgentCliKind::Gemini {
            if let (Some(source_home), Some(target_home)) =
                (self.source.parent(), self.target.parent())
            {
                let shared = source_home.join(".agents/skills");
                if shared.is_dir() {
                    mirror_assets(
                        &shared,
                        &target_home.join(".agents/skills"),
                        &mut budget,
                        started,
                    )?;
                }
            }
        }
        if self.kind == AgentCliKind::ClaudeCode && !self.target.join(".claude.json").exists() {
            let global = self.source.parent().map(|p| p.join(".claude.json"));
            if let Some(global) = global.filter(|p| p.is_file()) {
                copy_file(&global, &self.target.join(".claude.json"), &mut budget)?;
            }
        }
        Ok(())
    }
}

fn create_private_dir(path: &Path) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| format!("创建独立 Agent 配置失败：{e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn copy_file(source: &Path, target: &Path, budget: &mut u64) -> Result<(), String> {
    let size = fs::metadata(source).map_err(|e| e.to_string())?.len();
    *budget = budget
        .checked_sub(size)
        .ok_or("Agent 配置及资产超过 128 MiB，无法建立本次会话副本；请保留会话或精简资产后重试")?;
    fs::copy(source, target).map_err(|e| format!("复制本次 Agent 配置失败：{e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(source)
            .map_err(|e| e.to_string())?
            .permissions()
            .mode();
        fs::set_permissions(target, fs::Permissions::from_mode(0o600 | (mode & 0o111)))
            .map_err(|e| e.to_string())?;
    }
    #[cfg(windows)]
    super::super::shell_runtime::script::restrict_to_owner(target)?;
    Ok(())
}

// Asset packages are referenced by their original absolute locations; writable
// native registries are copied. Do not copy all package contents on every launch.
fn mirror_assets(
    source: &Path,
    target: &Path,
    budget: &mut u64,
    started: Instant,
) -> Result<(), String> {
    create_private_dir(target)?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        check_deadline(started)?;
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let target = target.join(entry.file_name());
        if path.is_dir() {
            #[cfg(unix)]
            let linked = std::os::unix::fs::symlink(&path, &target).is_ok();
            #[cfg(windows)]
            let linked = std::os::windows::fs::symlink_dir(&path, &target).is_ok();
            #[cfg(not(any(unix, windows)))]
            let linked = false;
            if !linked {
                copy_tree(&path, &target, budget, started, 0)?;
            }
        } else if path.is_file() {
            copy_file(&path, &target, budget)?;
        }
    }
    Ok(())
}

fn copy_tree(
    source: &Path,
    target: &Path,
    budget: &mut u64,
    started: Instant,
    depth: usize,
) -> Result<(), String> {
    if depth > 24 || started.elapsed() > Duration::from_secs(15) {
        return Err("建立独立资产副本耗时过长，请保留会话后重试".into());
    }
    create_private_dir(target)?;
    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
        check_deadline(started)?;
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        if path.is_dir() {
            copy_tree(&path, &target.join(name), budget, started, depth + 1)?;
        } else if path.is_file() {
            copy_file(&path, &target.join(name), budget)?;
        }
    }
    Ok(())
}

fn check_deadline(started: Instant) -> Result<(), String> {
    if started.elapsed() > Duration::from_secs(15) {
        return Err("建立独立资产副本耗时过长，请保留会话后重试".into());
    }
    Ok(())
}

fn rebase_codex_paths(source: &Path, target: &Path) -> Result<(), String> {
    let text = fs::read_to_string(target).map_err(|e| e.to_string())?;
    let mut document: toml::Value = text
        .parse()
        .map_err(|_| "Codex 配置格式无效，无法建立独立会话")?;
    let mut changed = false;
    let mut rebase = |value: Option<&mut toml::Value>| {
        if let Some(value) = value {
            if let Some(path) = value.as_str().filter(|path| Path::new(path).is_relative()) {
                *value = toml::Value::String(source.join(path).to_string_lossy().into_owned());
                changed = true;
            }
        }
    };
    for key in ["model_instructions_file", "model_catalog_json"] {
        rebase(document.get_mut(key));
    }
    if let Some(agents) = document
        .get_mut("agents")
        .and_then(toml::Value::as_table_mut)
    {
        for (_, agent) in agents.iter_mut() {
            rebase(agent.get_mut("config_file"));
        }
    }
    if changed {
        fs::write(
            target,
            toml::to_string_pretty(&document).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
