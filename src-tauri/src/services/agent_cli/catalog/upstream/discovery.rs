use crate::{
    models::{AgentCatalogAsset, AgentSkillOrigin},
    services::agent_cli::environment::mutation::GuardedFile,
};
use std::path::{Path, PathBuf};

pub(super) fn discover(item: &AgentCatalogAsset) -> Option<AgentSkillOrigin> {
    let home = crate::services::cli_paths::user_home()?;
    for binding in &item.bindings {
        let Some(path) = binding.native.path.as_deref() else {
            continue;
        };
        let path = PathBuf::from(path);
        let package = if path.file_name().is_some_and(|name| name == "SKILL.md") {
            path.parent()?.to_path_buf()
        } else {
            path
        };
        if let Some(origin) = git(&home, &package) {
            return Some(origin);
        }
        for directory in package
            .ancestors()
            .take(12)
            .filter(|path| path.starts_with(&home))
        {
            for file in [
                directory.join("skills-lock.json"),
                directory.join(".skill-lock.json"),
            ] {
                if let Some(origin) = lock(&home, &file, &item.name) {
                    return Some(origin);
                }
            }
        }
    }
    lock(&home, &home.join(".agents/.skill-lock.json"), &item.name)
}
fn read(root: &Path, path: &Path) -> Option<String> {
    let source = GuardedFile::capture_path(root, path, 2 * 1024 * 1024).ok()?;
    String::from_utf8(source.bytes()?.to_vec()).ok()
}
fn git(home: &Path, package: &Path) -> Option<AgentSkillOrigin> {
    for directory in package
        .ancestors()
        .take(16)
        .filter(|path| path.starts_with(home))
    {
        let dot_git = directory.join(".git");
        let git = if dot_git.is_dir() {
            dot_git
        } else {
            let Some(descriptor) = read(home, &dot_git) else {
                continue;
            };
            let Some(path) = descriptor.trim().strip_prefix("gitdir:").map(str::trim) else {
                continue;
            };
            let path = Path::new(path);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                directory.join(path)
            };
            let Ok(path) = path.canonicalize() else {
                continue;
            };
            if !path.starts_with(home) {
                continue;
            }
            path
        };
        let common = match read(home, &git.join("commondir")) {
            Some(relative) => {
                let Ok(path) = git.join(relative.trim()).canonicalize() else {
                    continue;
                };
                if !path.starts_with(home) {
                    continue;
                }
                path
            }
            None => git.clone(),
        };
        let config = match read(home, &common.join("config")) {
            Some(value) => value,
            None => continue,
        };
        let mut origin_section = false;
        let mut url = None;
        for line in config.lines().map(str::trim) {
            if line.starts_with('[') {
                origin_section = line == "[remote \"origin\"]";
            }
            if origin_section {
                if let Some(value) = line
                    .strip_prefix("url")
                    .and_then(|value| value.trim_start().strip_prefix('='))
                {
                    url = Some(value.trim().to_owned());
                }
            }
        }
        let mut url = url?;
        if let Some(path) = url.strip_prefix("git@github.com:") {
            url = format!("https://github.com/{path}");
        }
        let head = read(home, &git.join("HEAD")).unwrap_or_default();
        let reference = head
            .trim()
            .strip_prefix("ref: refs/heads/")
            .unwrap_or("")
            .to_owned();
        return super::github::normalize(AgentSkillOrigin {
            repository: url,
            reference,
            directory: package
                .strip_prefix(directory)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/"),
            revision: None,
        })
        .ok();
    }
    None
}
fn lock(home: &Path, path: &Path, name: &str) -> Option<AgentSkillOrigin> {
    let text = read(home, path)?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let skill = value.get("skills")?.get(name)?;
    let source = skill
        .get("sourceUrl")
        .or_else(|| skill.get("source"))?
        .as_str()?;
    let repository = if source.contains("://") {
        source.to_owned()
    } else {
        format!("https://github.com/{source}")
    };
    let directory = skill
        .get("skillPath")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim_end_matches("/SKILL.md")
        .to_owned();
    super::github::normalize(AgentSkillOrigin {
        repository,
        reference: skill
            .get("ref")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_owned(),
        directory,
        revision: None,
    })
    .ok()
}
