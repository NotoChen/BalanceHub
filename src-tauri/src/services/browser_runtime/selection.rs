use super::{manifest, root_dir, BrowserInfo};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use tauri::AppHandle;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub(crate) enum BrowserSelection {
    System { path: PathBuf },
    Managed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct FileStamp {
    path: PathBuf,
    bytes: u64,
    modified: u128,
}

impl FileStamp {
    fn read(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        Some(Self {
            path: fs::canonicalize(path).ok()?,
            bytes: metadata.len(),
            modified: metadata
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_nanos(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Validation {
    runtime_version: String,
    browser: FileStamp,
    node: FileStamp,
    playwright: FileStamp,
    pub browser_version: String,
}

impl Validation {
    pub fn capture(
        directory: &Path,
        browser: &BrowserInfo,
        browser_version: String,
    ) -> Option<Self> {
        Some(Self {
            runtime_version: manifest::manifest().version.clone(),
            browser: FileStamp::read(&browser.path)?,
            node: FileStamp::read(&directory.join(manifest::node_name()))?,
            playwright: FileStamp::read(
                &directory.join("node_modules/playwright-core/package.json"),
            )?,
            browser_version,
        })
    }

    pub fn matches(&self, directory: &Path, browser: &BrowserInfo) -> bool {
        Self::capture(directory, browser, self.browser_version.clone()).as_ref() == Some(self)
    }
}

pub(super) fn save(
    app: &AppHandle,
    selected: BrowserSelection,
    validation: Validation,
) -> Result<(), String> {
    let (mut installed, _) = super::detection::installed(app).ok_or("浏览器辅助组件尚未安装")?;
    installed.selected = Some(selected);
    installed.validation = Some(validation);
    save_installed(app, &installed)
}

pub(super) fn save_installed(
    app: &AppHandle,
    installed: &super::detection::Installed,
) -> Result<(), String> {
    let root = root_dir(app)?;
    fs::create_dir_all(&root).map_err(|_| "无法保存浏览器选择")?;
    let pending = root.join("active.json.tmp");
    let result = (|| {
        let mut file = fs::File::create(&pending).map_err(|_| "无法保存浏览器选择")?;
        file.write_all(&serde_json::to_vec(installed).map_err(|_| "无法编码浏览器选择")?)
            .map_err(|_| "无法保存浏览器选择")?;
        file.sync_all().map_err(|_| "无法保存浏览器选择")?;
        fs::rename(&pending, root.join("active.json")).map_err(|_| "无法更新浏览器选择")
    })();
    if result.is_err() {
        let _ = fs::remove_file(pending);
    }
    result.map_err(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_browser_validation_is_invalidated_by_browser_or_runtime_changes() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path();
        let browser = BrowserInfo {
            name: "Fixture".into(),
            path: directory.join("browser"),
            managed: false,
            version: None,
        };
        fs::create_dir_all(directory.join("node_modules/playwright-core")).unwrap();
        let node = directory.join(manifest::node_name());
        let package = directory.join("node_modules/playwright-core/package.json");
        fs::write(&browser.path, "browser-v1").unwrap();
        fs::write(&node, "node-v1").unwrap();
        fs::write(&package, "package-v1").unwrap();
        let validation = Validation::capture(directory, &browser, "140.0".into()).unwrap();
        let restored: Validation =
            serde_json::from_slice(&serde_json::to_vec(&validation).unwrap()).unwrap();
        assert!(restored.matches(directory, &browser));
        fs::write(&browser.path, "browser-updated-version").unwrap();
        assert!(!restored.matches(directory, &browser));
        let updated = Validation::capture(directory, &browser, "141.0".into()).unwrap();
        fs::write(&node, "node-updated-version").unwrap();
        assert!(!updated.matches(directory, &browser));
        let updated = Validation::capture(directory, &browser, "141.0".into()).unwrap();
        fs::remove_file(&package).unwrap();
        assert!(!updated.matches(directory, &browser));
    }
}
