use crate::models::{WorkspaceDirectoryEntry, WorkspaceDirectoryListing};
use std::{
    env, fs,
    path::{Path, PathBuf},
};

pub fn browse(path: Option<&str>) -> Result<WorkspaceDirectoryListing, String> {
    let home = home_dir().ok_or_else(|| "无法定位用户目录".to_string())?;
    let requested = path
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.clone());
    let current = normalize_directory(&requested)?;
    let home = normalize_directory(&home).unwrap_or(home);
    let mut entries = fs::read_dir(&current)
        .map_err(|err| format!("无法读取工作空间目录({}): {err}", current.display()))?
        .flatten()
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if !file_type.is_dir() && !file_type.is_symlink() {
                return None;
            }
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            Some(WorkspaceDirectoryEntry {
                hidden: name.starts_with('.'),
                name,
                path: path.to_string_lossy().to_string(),
            })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(WorkspaceDirectoryListing {
        current_path: current.to_string_lossy().to_string(),
        parent_path: current
            .parent()
            .filter(|parent| *parent != current)
            .map(|parent| parent.to_string_lossy().to_string()),
        home_path: home.to_string_lossy().to_string(),
        entries,
    })
}

pub fn normalize_directory(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path)
        .map_err(|err| format!("工作空间目录不可用({}): {err}", path.display()))?;
    if !canonical.is_dir() {
        return Err("所选工作空间不是文件夹".to_string());
    }
    Ok(canonical)
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        env::var_os("USERPROFILE")
            .or_else(|| {
                let mut home = env::var_os("HOMEDRIVE")?;
                home.push(env::var_os("HOMEPATH")?);
                Some(home)
            })
            .map(PathBuf::from)
    }

    #[cfg(not(target_os = "windows"))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browse_lists_directories_and_marks_hidden_entries() {
        let root = env::temp_dir().join(format!(
            "balancehub-workspace-browser-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("alpha")).unwrap();
        fs::create_dir_all(root.join(".hidden")).unwrap();
        fs::write(root.join("file.txt"), "ignored").unwrap();

        let listing = browse(root.to_str()).unwrap();
        assert_eq!(listing.entries.len(), 2);
        assert_eq!(listing.entries[0].name, ".hidden");
        assert!(listing.entries[0].hidden);
        assert_eq!(listing.entries[1].name, "alpha");
        assert!(!listing.entries[1].hidden);

        let _ = fs::remove_dir_all(root);
    }
}
