use super::{
    manifest::{manifest, node_name, target},
    root_dir, BrowserInfo,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};
use tauri::AppHandle;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Installed {
    pub version: String,
    pub directory: String,
}

pub(super) fn installed(app: &AppHandle) -> Option<(Installed, PathBuf)> {
    let root = root_dir(app).ok()?;
    let state: Installed =
        serde_json::from_slice(&fs::read(root.join("active.json")).ok()?).ok()?;
    if !safe_directory(&state.directory) {
        return None;
    }
    let directory = root.join(&state.directory);
    Some((state, directory))
}

fn safe_directory(value: &str) -> bool {
    !value.is_empty()
        && Path::new(value)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        && Path::new(value).components().count() == 1
}

pub(super) fn core_ready(directory: &Path) -> bool {
    executable(&directory.join(node_name()))
        && directory
            .join("node_modules/playwright-core/package.json")
            .is_file()
}

pub(super) fn managed_browser(directory: &Path) -> Option<BrowserInfo> {
    let path = directory
        .join("chromium")
        .join(&target().ok()?.browser_executable);
    executable(&path).then(|| BrowserInfo {
        name: "独立 Chromium".to_string(),
        path,
        managed: true,
        version: Some(manifest().browser_version.clone()),
    })
}

pub(super) fn system_browser() -> Option<BrowserInfo> {
    candidates().into_iter().find_map(|(name, path)| {
        executable(&path).then(|| BrowserInfo {
            name: name.to_string(),
            path,
            managed: false,
            version: None,
        })
    })
}

pub(super) fn executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return false;
        }
    }
    true
}

fn candidates() -> Vec<(&'static str, PathBuf)> {
    let mut result = Vec::new();
    if cfg!(target_os = "macos") {
        let mut roots = vec![PathBuf::from("/Applications")];
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join("Applications"));
        }
        for root in roots {
            for (name, relative) in [
                (
                    "Google Chrome",
                    "Google Chrome.app/Contents/MacOS/Google Chrome",
                ),
                (
                    "Microsoft Edge",
                    "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
                ),
                ("Chromium", "Chromium.app/Contents/MacOS/Chromium"),
                ("Brave", "Brave Browser.app/Contents/MacOS/Brave Browser"),
            ] {
                result.push((name, root.join(relative)));
            }
        }
    } else if cfg!(windows) {
        for root in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"]
            .into_iter()
            .filter_map(std::env::var_os)
        {
            for (name, relative) in [
                ("Google Chrome", "Google/Chrome/Application/chrome.exe"),
                ("Microsoft Edge", "Microsoft/Edge/Application/msedge.exe"),
                ("Brave", "BraveSoftware/Brave-Browser/Application/brave.exe"),
            ] {
                result.push((name, PathBuf::from(&root).join(relative)));
            }
        }
    } else {
        for (name, executable) in [
            ("Google Chrome", "google-chrome"),
            ("Google Chrome", "google-chrome-stable"),
            ("Microsoft Edge", "microsoft-edge"),
            ("Chromium", "chromium"),
            ("Chromium", "chromium-browser"),
            ("Brave", "brave-browser"),
        ] {
            for root in ["/usr/bin", "/usr/local/bin", "/snap/bin"] {
                result.push((name, PathBuf::from(root).join(executable)));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_pointer_cannot_escape_the_component_directory() {
        assert!(safe_directory("runtime-123"));
        for invalid in ["", "../runtime", "/tmp/runtime", "a/b", ".", ".."] {
            assert!(!safe_directory(invalid));
        }
    }
}
