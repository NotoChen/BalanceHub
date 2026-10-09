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
#[cfg(windows)]
mod windows;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Installed {
    pub version: String,
    pub directory: String,
    pub selected: Option<super::selection::BrowserSelection>,
    pub validation: Option<super::selection::Validation>,
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

pub(super) fn system_browsers() -> Vec<BrowserInfo> {
    let mut seen = std::collections::HashSet::new();
    candidates()
        .into_iter()
        .filter_map(|(name, path)| {
            if !executable(&path) {
                return None;
            }
            let canonical = fs::canonicalize(&path)
                .unwrap_or_else(|_| path.clone())
                .to_string_lossy()
                .into_owned();
            let key = if cfg!(windows) {
                canonical.to_lowercase()
            } else {
                canonical
            };
            seen.insert(key).then_some(BrowserInfo {
                name,
                path,
                managed: false,
                version: None,
            })
        })
        .collect()
}

pub(crate) fn inspect(path: &Path) -> Result<BrowserInfo, String> {
    #[cfg(target_os = "macos")]
    let path = if path.extension().is_some_and(|ext| ext == "app") {
        bundle_executable(path)?
    } else {
        path.to_path_buf()
    };
    #[cfg(not(target_os = "macos"))]
    let path = path.to_path_buf();
    if !path.is_absolute() || !executable(&path) {
        return Err("未找到可执行的浏览器程序，请选择浏览器应用或其可执行文件".into());
    }
    let name = browser_name(&path).unwrap_or_else(|| {
        path.file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    });
    Ok(BrowserInfo {
        name,
        path,
        managed: false,
        version: None,
    })
}

#[cfg(target_os = "macos")]
fn bundle_executable(bundle: &Path) -> Result<PathBuf, String> {
    use crate::platform::process::run_command_with_output_timeout;
    let mut command = std::process::Command::new("/usr/bin/plutil");
    command
        .args(["-extract", "CFBundleExecutable", "raw", "-o", "-"])
        .arg(bundle.join("Contents/Info.plist"));
    let output =
        run_command_with_output_timeout(&mut command, std::time::Duration::from_secs(2), 1024)
            .map_err(|_| "无法读取浏览器应用，请选择其可执行文件")?;
    let name = output.stdout.trim();
    if output.timed_out
        || !output.status.is_some_and(|status| status.success())
        || !safe_directory(name)
    {
        return Err("浏览器应用信息无效，请选择其可执行文件".into());
    }
    Ok(bundle.join("Contents/MacOS").join(name))
}

fn browser_name(path: &Path) -> Option<String> {
    let value = path.to_string_lossy().to_lowercase();
    let name = if value.contains("brave") {
        "Brave"
    } else if value.contains("msedge")
        || value.contains("microsoft edge")
        || value.contains("microsoft-edge")
    {
        "Microsoft Edge"
    } else if value.contains("chromium") {
        "Chromium"
    } else if value.contains("chrome") {
        "Google Chrome"
    } else if value.contains("vivaldi") {
        "Vivaldi"
    } else if value.contains("opera") {
        "Opera"
    } else {
        return None;
    };
    Some(name.to_string())
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

fn candidates() -> Vec<(String, PathBuf)> {
    let mut result = Vec::new();
    #[cfg(windows)]
    result.extend(windows::candidates());
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
                result.push((name.to_string(), root.join(relative)));
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
                ("Chromium", "Chromium/Application/chrome.exe"),
            ] {
                result.push((name.to_string(), PathBuf::from(&root).join(relative)));
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
            let roots = std::env::var_os("PATH")
                .map(|value| std::env::split_paths(&value).collect::<Vec<_>>())
                .unwrap_or_default();
            for root in roots
                .into_iter()
                .chain(["/usr/bin", "/usr/local/bin", "/snap/bin"].map(PathBuf::from))
            {
                if root.is_absolute() {
                    result.push((name.to_string(), root.join(executable)));
                }
            }
        }
    }
    result
}

#[cfg(any(windows, test))]
fn registry_executable(value: &str) -> Option<PathBuf> {
    let value = value.trim();
    let path = if let Some(quoted) = value.strip_prefix('"') {
        quoted.split_once('"')?.0
    } else {
        let end = value
            .as_bytes()
            .windows(4)
            .enumerate()
            .find(|(offset, part)| {
                part.eq_ignore_ascii_case(b".exe")
                    && value
                        .as_bytes()
                        .get(offset + 4)
                        .is_none_or(|next| next.is_ascii_whitespace() || *next == b'"')
            })?
            .0
            + 4;
        &value[..end]
    };
    if !path.to_lowercase().ends_with(".exe") {
        return None;
    }
    let mut expanded = String::new();
    let mut remaining = path;
    while let Some(start) = remaining.find('%') {
        expanded.push_str(&remaining[..start]);
        let suffix = &remaining[start + 1..];
        let end = suffix.find('%')?;
        expanded.push_str(&std::env::var_os(&suffix[..end])?.to_string_lossy());
        remaining = &suffix[end + 1..];
    }
    expanded.push_str(remaining);
    Some(PathBuf::from(expanded))
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

    #[test]
    fn registry_paths_preserve_spaces_unicode_and_strip_launch_arguments() {
        for command in [
            r#""D:\软件\Google Chrome\chrome.exe" --single-argument %1"#,
            r"D:\软件\Google Chrome\chrome.exe --profile-directory=Default",
        ] {
            assert_eq!(
                registry_executable(command),
                Some(PathBuf::from(r"D:\软件\Google Chrome\chrome.exe"))
            );
        }
        assert_eq!(
            registry_executable(r"D:\İ用户\Chrome\chrome.EXE --new-window"),
            Some(PathBuf::from(r"D:\İ用户\Chrome\chrome.EXE"))
        );
        assert_eq!(
            registry_executable(r"D:\Browser.exe\Google Chrome\chrome.exe --new-window"),
            Some(PathBuf::from(r"D:\Browser.exe\Google Chrome\chrome.exe"))
        );
        for invalid in [
            "",
            "not a browser",
            r#""D:\Chrome\chrome.exe"#,
            r"%BALANCEHUB_MISSING_BROWSER_ROOT%\chrome.exe",
        ] {
            assert!(registry_executable(invalid).is_none());
        }
    }

    #[test]
    fn custom_browser_paths_are_checked_and_not_silently_replaced() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("custom-chromium");
        fs::write(&path, "fixture").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert!(inspect(&path).is_err());
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        }
        let browser = inspect(&path).unwrap();
        assert_eq!(browser.path, path);
        assert_eq!(browser.name, "Chromium");
        assert!(!browser.managed);
        fs::remove_file(path).unwrap();
        assert!(inspect(&browser.path).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn renamed_app_bundles_use_their_declared_executable() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let bundle = root.path().join("Renamed browser.app");
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>Chromium</string></dict></plist>"#,
        ).unwrap();
        let executable = bundle.join("Contents/MacOS/Chromium");
        fs::write(&executable, "fixture").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(inspect(&bundle).unwrap().path, executable);
    }
}
