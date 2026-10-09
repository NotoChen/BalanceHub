use super::{
    detection::Installed,
    manifest::{manifest, node_name, write_worker_files, Artifact, Target},
    publish, root_dir, selected_browser,
    selection::{self, Validation},
    BrowserSelection,
};
use crate::{models::AppSettings, network};
use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::io::Read;
use std::{
    fs,
    path::{Component, Path},
    time::{Duration, Instant},
};
use tauri::AppHandle;
use tokio::{io::AsyncWriteExt, sync::watch};

pub(super) async fn run(
    app: &AppHandle,
    settings: &AppSettings,
    target: &Target,
    selection: BrowserSelection,
    repair: bool,
    cancelled: &mut watch::Receiver<bool>,
) -> Result<(), String> {
    let root = root_dir(app)?;
    fs::create_dir_all(&root).map_err(|_| "无法创建组件目录")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
            .map_err(|_| "无法设置组件目录权限")?;
    }
    let stamp = crate::util::unix_millis();
    let staging = root.join(format!(".staging-{stamp}"));
    fs::create_dir_all(&staging).map_err(|_| "无法准备组件下载目录")?;
    // Downloads and the executable probe have their own timeouts. Always await
    // extraction (which checks cancellation per entry) before deleting staging;
    // dropping a spawn_blocking future would leave it writing into a removed dir.
    let reusable = super::detection::installed(app)
        .filter(|(installed, _)| !repair && installed.version == manifest().version)
        .map(|(_, path)| path);
    let result = prepare(
        app,
        settings,
        target,
        &selection,
        reusable.as_deref(),
        cancelled,
        &staging,
    )
    .await;
    let version = match result {
        Ok(version) => version,
        Err(message) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(message);
        }
    };
    if *cancelled.borrow() {
        let _ = fs::remove_dir_all(&staging);
        return Err("安装已取消".to_string());
    }
    // Activate a complete new directory through one small pointer replacement.
    let name = format!("runtime-{stamp}");
    let directory = root.join(&name);
    let old = super::detection::installed(app).map(|(_, directory)| directory);
    if fs::rename(&staging, &directory).is_err() {
        let _ = fs::remove_dir_all(&staging);
        return Err("无法启用新组件".into());
    }
    let activate = (|| -> Result<(), String> {
        let browser = selected_browser(&selection, &directory)?;
        let validation =
            Validation::capture(&directory, &browser, version).ok_or("组件文件已变更，请重试")?;
        selection::save_installed(
            app,
            &Installed {
                version: manifest().version.clone(),
                directory: name,
                selected: Some(selection),
                validation: Some(validation),
            },
        )
    })();
    if let Err(message) = activate {
        let _ = fs::remove_dir_all(directory);
        return Err(message);
    }
    if let Some(old) = old.filter(|old| old != &directory) {
        let _ = fs::remove_dir_all(old);
    }
    Ok(())
}

async fn prepare(
    app: &AppHandle,
    settings: &AppSettings,
    target: &Target,
    selection: &BrowserSelection,
    reusable: Option<&Path>,
    cancelled: &mut watch::Receiver<bool>,
    staging: &Path,
) -> Result<String, String> {
    let include_browser = matches!(selection, BrowserSelection::Managed);
    let client = network::build_download_client(settings)?;
    if let Some(existing) = reusable.filter(|path| super::detection::core_ready(path)) {
        publish(app, "installing", "正在复用已安装的辅助组件", Some(0.18));
        let existing = existing.to_path_buf();
        let destination = staging.to_path_buf();
        let cancel = cancelled.clone();
        tokio::task::spawn_blocking(move || {
            clone_component(
                &existing.join(node_name()),
                &destination.join(node_name()),
                &cancel,
            )?;
            clone_component(
                &existing.join("node_modules/playwright-core"),
                &destination.join("node_modules/playwright-core"),
                &cancel,
            )
        })
        .await
        .map_err(|_| "复用辅助组件失败")??;
    } else {
        let node_archive = staging.join("node.download");
        download(
            app,
            &client,
            &target.node,
            &node_archive,
            cancelled,
            (0.0, if include_browser { 0.18 } else { 0.75 }),
            "正在下载浏览器运行组件",
        )
        .await?;
        let target_copy = target.clone();
        let stage = staging.to_path_buf();
        let cancel = cancelled.clone();
        tokio::task::spawn_blocking(move || {
            check_cancelled(&cancel)?;
            if target_copy.node_prefix.is_empty() {
                fs::rename(&node_archive, stage.join(node_name()))
                    .map_err(|_| "无法安装运行环境")?;
            } else {
                extract_tar(
                    &node_archive,
                    &stage,
                    &target_copy.node_prefix,
                    true,
                    &cancel,
                )?;
                fs::remove_file(&node_archive).map_err(|_| "无法清理组件下载文件")?;
            }
            set_executable(&stage.join(node_name()))
        })
        .await
        .map_err(|_| "运行环境解压任务异常")??;
        let playwright_archive = staging.join("playwright.tgz");
        download(
            app,
            &client,
            &manifest().playwright,
            &playwright_archive,
            cancelled,
            (
                if include_browser { 0.18 } else { 0.75 },
                if include_browser { 0.05 } else { 0.15 },
            ),
            "正在下载浏览器控制组件",
        )
        .await?;
        let stage = staging.to_path_buf();
        let cancel = cancelled.clone();
        tokio::task::spawn_blocking(move || {
            extract_tar(
                &playwright_archive,
                &stage.join("node_modules/playwright-core"),
                "package",
                false,
                &cancel,
            )?;
            fs::remove_file(playwright_archive).map_err(|_| "无法清理组件下载文件".to_string())
        })
        .await
        .map_err(|_| "控制组件解压任务异常")??;
    }
    if include_browser {
        if let Some(existing) =
            reusable.filter(|path| super::detection::managed_browser(path).is_some())
        {
            publish(app, "installing", "正在复用独立 Chromium", Some(0.9));
            let existing = existing.join("chromium");
            let destination = staging.join("chromium");
            let cancel = cancelled.clone();
            tokio::task::spawn_blocking(move || clone_component(&existing, &destination, &cancel))
                .await
                .map_err(|_| "复用浏览器失败")??;
        } else {
            let browser_archive = staging.join("chromium.zip");
            download(
                app,
                &client,
                &target.browser,
                &browser_archive,
                cancelled,
                (0.23, 0.67),
                "正在下载独立 Chromium 浏览器",
            )
            .await?;
            publish(app, "installing", "正在解压独立浏览器", Some(0.91));
            let destination = staging.join("chromium");
            let cancel = cancelled.clone();
            tokio::task::spawn_blocking(move || {
                extract_zip(&browser_archive, &destination, &cancel)?;
                fs::remove_file(browser_archive).map_err(|_| "无法清理浏览器下载文件".to_string())
            })
            .await
            .map_err(|_| "浏览器解压任务异常")??;
        }
        set_executable(&staging.join("chromium").join(&target.browser_executable))?;
    }
    check_cancelled(cancelled)?;
    write_worker_files(staging)?;
    publish(app, "installing", "正在验证浏览器启动", Some(0.96));
    let browser = selected_browser(selection, staging)?;
    super::probe::run(app, staging, &browser, cancelled).await
}

fn clone_component(
    source: &Path,
    destination: &Path,
    cancelled: &watch::Receiver<bool>,
) -> Result<(), String> {
    fn copy(
        root: &Path,
        source: &Path,
        destination: &Path,
        cancelled: &watch::Receiver<bool>,
    ) -> Result<(), String> {
        check_cancelled(cancelled)?;
        source.strip_prefix(root).map_err(|_| "组件路径越界")?;
        let metadata =
            fs::symlink_metadata(source).map_err(|_| "已有组件不完整，请选择修复组件")?;
        if metadata.is_dir() {
            fs::create_dir_all(destination).map_err(|_| "无法复用组件目录")?;
            for entry in fs::read_dir(source).map_err(|_| "无法读取已有组件")? {
                let entry = entry.map_err(|_| "无法读取已有组件")?;
                copy(
                    root,
                    &entry.path(),
                    &destination.join(entry.file_name()),
                    cancelled,
                )?;
            }
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|_| "无法复用组件目录")?;
            }
            if metadata.file_type().is_symlink() {
                #[cfg(unix)]
                {
                    let target =
                        fs::read_link(source).map_err(|_| "已有组件链接无效，请修复组件")?;
                    if !link_stays_inside(
                        source.strip_prefix(root).map_err(|_| "组件路径无效")?,
                        &target,
                    ) {
                        return Err("已有组件链接越界，请修复组件".into());
                    }
                    std::os::unix::fs::symlink(target, destination)
                        .map_err(|_| "无法复用组件链接")?;
                }
                #[cfg(not(unix))]
                return Err("已有组件包含不支持的链接，请修复组件".into());
            } else if metadata.is_file() {
                if fs::hard_link(source, destination).is_err() {
                    fs::copy(source, destination)
                        .map_err(|_| "无法复用组件文件，请检查磁盘空间")?;
                }
            } else {
                return Err("已有组件文件类型无效，请修复组件".into());
            }
        }
        Ok(())
    }
    copy(source, source, destination, cancelled)
}

async fn download(
    app: &AppHandle,
    client: &reqwest::Client,
    artifact: &Artifact,
    path: &Path,
    cancelled: &mut watch::Receiver<bool>,
    range: (f64, f64),
    message: &str,
) -> Result<(), String> {
    check_cancelled(cancelled)?;
    publish(app, "installing", message, Some(range.0));
    let mut response = tokio::select! {
        response = client.get(&artifact.url).send() => response.map_err(|_| "组件下载连接失败，请检查网络或代理后重试")?,
        _ = cancelled.changed() => return Err("安装已取消".to_string()),
    };
    if !response.status().is_success() {
        return Err(format!("组件下载失败：HTTP {}", response.status().as_u16()));
    }
    let total = response.content_length().unwrap_or(artifact.size);
    let mut file = tokio::fs::File::create(path)
        .await
        .map_err(|_| "无法写入组件下载文件")?;
    let mut digest = Sha256::new();
    let mut received = 0u64;
    let mut last_update = Instant::now();
    loop {
        check_cancelled(cancelled)?;
        let chunk = tokio::select! {
            chunk = response.chunk() => chunk.map_err(|_| "组件下载中断，请重试")?,
            _ = cancelled.changed() => return Err("安装已取消".to_string()),
        };
        let Some(chunk) = chunk else {
            break;
        };
        received += chunk.len() as u64;
        if received > 600 * 1024 * 1024 {
            return Err("下载文件超过组件大小限制".to_string());
        }
        digest.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|_| "组件写入失败，请检查剩余磁盘空间")?;
        if last_update.elapsed() >= Duration::from_millis(200) {
            let progress = (received as f64 / total.max(1) as f64).min(1.0);
            publish(
                app,
                "installing",
                message,
                Some(range.0 + range.1 * progress),
            );
            last_update = Instant::now();
        }
    }
    file.flush().await.map_err(|_| "无法保存组件文件")?;
    if format!("{:x}", digest.finalize()) != artifact.sha256 {
        return Err("组件完整性校验失败，已拒绝安装，请重试".to_string());
    }
    Ok(())
}

fn check_cancelled(cancelled: &watch::Receiver<bool>) -> Result<(), String> {
    if *cancelled.borrow() {
        Err("安装已取消".to_string())
    } else {
        Ok(())
    }
}

fn safe_relative(path: &Path) -> bool {
    path.components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

fn extract_tar(
    archive: &Path,
    destination: &Path,
    prefix: &str,
    node_only: bool,
    cancelled: &watch::Receiver<bool>,
) -> Result<(), String> {
    let archive = fs::File::open(archive).map_err(|_| "无法读取组件压缩包")?;
    let mut archive = tar::Archive::new(GzDecoder::new(archive));
    for entry in archive.entries().map_err(|_| "组件压缩包损坏")? {
        check_cancelled(cancelled)?;
        let mut entry = entry.map_err(|_| "组件压缩包损坏")?;
        let path = entry.path().map_err(|_| "组件文件路径无效")?.into_owned();
        let relative = path
            .strip_prefix(prefix)
            .map_err(|_| "组件压缩包目录无效")?;
        if !safe_relative(relative) {
            return Err("组件压缩包包含无效路径".to_string());
        }
        let output = if node_only {
            if relative == Path::new("bin/node") {
                destination.join(node_name())
            } else if relative == Path::new("LICENSE") {
                destination.join("NODE-LICENSE")
            } else {
                continue;
            }
        } else {
            destination.join(relative)
        };
        if entry.header().entry_type().is_dir() {
            fs::create_dir_all(output).map_err(|_| "无法创建组件子目录")?;
            continue;
        }
        if !entry.header().entry_type().is_file() {
            return Err("组件压缩包包含不支持的文件类型".to_string());
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|_| "无法创建组件子目录")?;
        }
        entry
            .unpack(output)
            .map_err(|_| "无法解压组件，请检查磁盘空间")?;
    }
    Ok(())
}

fn extract_zip(
    archive: &Path,
    destination: &Path,
    cancelled: &watch::Receiver<bool>,
) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|_| "无法读取浏览器压缩包")?;
    let mut archive = zip::ZipArchive::new(file).map_err(|_| "浏览器压缩包损坏")?;
    #[cfg(unix)]
    let mut links = Vec::new();
    let mut total = 0u64;
    for index in 0..archive.len() {
        check_cancelled(cancelled)?;
        let mut entry = archive.by_index(index).map_err(|_| "浏览器压缩包损坏")?;
        let relative = entry.enclosed_name().ok_or("浏览器压缩包路径无效")?;
        let output = destination.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&output).map_err(|_| "无法创建浏览器目录")?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent).map_err(|_| "无法创建浏览器目录")?;
        }
        #[cfg(unix)]
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            let mut target = String::new();
            entry
                .take(4096)
                .read_to_string(&mut target)
                .map_err(|_| "浏览器符号链接无效")?;
            if !link_stays_inside(&relative, Path::new(&target)) {
                return Err("浏览器符号链接越界".to_string());
            }
            links.push((target, output));
            continue;
        }
        total = total
            .checked_add(entry.size())
            .ok_or("浏览器解压大小无效")?;
        if total > 2 * 1024 * 1024 * 1024 {
            return Err("浏览器解压大小超过限制".to_string());
        }
        let mut file = fs::File::create(&output).map_err(|_| "无法写入浏览器文件")?;
        std::io::copy(&mut entry, &mut file).map_err(|_| "浏览器解压失败，请检查磁盘空间")?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&output, fs::Permissions::from_mode(mode & 0o777))
                .map_err(|_| "无法设置浏览器文件权限")?;
        }
    }
    #[cfg(unix)]
    for (target, output) in links {
        std::os::unix::fs::symlink(target, output).map_err(|_| "无法创建浏览器符号链接")?;
    }
    Ok(())
}

#[cfg(any(unix, test))]
fn link_stays_inside(relative: &Path, target: &Path) -> bool {
    let mut depth = relative
        .parent()
        .map_or(0, |path| path.components().count());
    for part in target.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    true
}

fn set_executable(path: &Path) -> Result<(), String> {
    if !path.is_file() {
        return Err("组件缺少浏览器或运行环境程序".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
            .map_err(|_| "无法设置组件可执行权限")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_paths_and_framework_links_stay_in_the_component() {
        assert!(safe_relative(Path::new("lib/server/index.js")));
        assert!(!safe_relative(Path::new("../outside")));
        assert!(link_stays_inside(
            Path::new("Chromium.app/Versions/Current"),
            Path::new("A")
        ));
        assert!(!link_stays_inside(
            Path::new("a/link"),
            Path::new("../../outside")
        ));
    }

    #[test]
    fn component_reuse_preserves_source_and_obeys_cancellation() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let destination = root.path().join("destination");
        fs::create_dir_all(source.join("lib")).unwrap();
        fs::write(source.join("lib/driver.js"), "fixture-driver").unwrap();
        let (sender, cancelled) = watch::channel(false);
        clone_component(&source, &destination, &cancelled).unwrap();
        assert_eq!(
            fs::read(destination.join("lib/driver.js")).unwrap(),
            b"fixture-driver"
        );
        fs::remove_dir_all(&destination).unwrap();
        assert_eq!(
            fs::read(source.join("lib/driver.js")).unwrap(),
            b"fixture-driver"
        );
        sender.send(true).unwrap();
        assert!(clone_component(&source, &destination, &cancelled).is_err());
        assert!(!destination.exists());
    }

    #[cfg(unix)]
    #[test]
    fn component_reuse_keeps_framework_links_and_rejects_external_links() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir_all(source.join("Versions/A")).unwrap();
        fs::write(source.join("Versions/A/browser"), "fixture-browser").unwrap();
        std::os::unix::fs::symlink("A", source.join("Versions/Current")).unwrap();
        let (_sender, cancelled) = watch::channel(false);
        let destination = root.path().join("destination");
        clone_component(&source, &destination, &cancelled).unwrap();
        assert_eq!(
            fs::read(destination.join("Versions/Current/browser")).unwrap(),
            b"fixture-browser"
        );
        std::os::unix::fs::symlink("../outside", source.join("escape")).unwrap();
        assert!(clone_component(&source, &root.path().join("rejected"), &cancelled).is_err());
    }
}
