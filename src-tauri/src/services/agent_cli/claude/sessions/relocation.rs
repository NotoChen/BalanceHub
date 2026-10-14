//! Claude project path encoding and exact metadata-only relocation edits.
use crate::services::agent_cli::{
    config_support::{rewrite_json_document, rewrite_json_string_fields},
    contracts::{
        SessionProjectRelocationAdapter, SessionProjectRelocationPaths, SessionRelocationFileKind,
    },
};
use serde_json::Value;
use std::path::Path;

pub(super) const ADAPTER: SessionProjectRelocationAdapter = SessionProjectRelocationAdapter {
    paths,
    file_kind,
    transform,
};

fn paths(
    root: &Path,
    locator: &Path,
    from: &str,
    to: &str,
) -> Result<SessionProjectRelocationPaths, String> {
    let source_name = super::encode_project_path(Path::new(from));
    let target_name = super::encode_project_path(Path::new(to));
    if source_name.len() > 200 || target_name.len() > 200 {
        return Err("Claude 对超长项目路径使用额外哈希，尚不能可靠生成目标目录名".to_owned());
    }
    if source_name.eq_ignore_ascii_case(&target_name) {
        return Err("两个项目路径映射到同一 Claude 历史目录，无需迁移".to_owned());
    }
    let source = root.join("projects").join(source_name);
    if locator.parent() != Some(source.as_path())
        || locator.extension().is_none_or(|ext| ext != "jsonl")
    {
        return Err("请选择此项目的主会话；其原始目录必须与 Claude 项目记录一致".to_owned());
    }
    Ok(SessionProjectRelocationPaths {
        source,
        destination: root.join("projects").join(target_name),
        from: from.to_owned(),
        to: to.to_owned(),
    })
}
fn file_kind(relative: &Path) -> SessionRelocationFileKind {
    if relative.extension().is_some_and(|ext| ext == "jsonl") {
        if relative.components().count() == 1 {
            return SessionRelocationFileKind::MainTranscript;
        }
        if relative
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "subagents")
        {
            return SessionRelocationFileKind::Transcript;
        }
    }
    if relative == Path::new("sessions-index.json") {
        SessionRelocationFileKind::Document
    } else {
        SessionRelocationFileKind::Copy
    }
}
fn remap(value: &str, from: &str, to: &str) -> Option<String> {
    if value == from {
        return Some(to.to_owned());
    }
    let suffix = value.strip_prefix(from)?;
    (suffix.starts_with('/') || suffix.starts_with('\\')).then(|| format!("{to}{suffix}"))
}
fn transform(
    relative: &Path,
    bytes: &[u8],
    paths: &SessionProjectRelocationPaths,
    saw_origin: bool,
) -> Result<(Vec<u8>, bool), String> {
    let kind = file_kind(relative);
    if kind == SessionRelocationFileKind::Copy {
        return Ok((bytes.to_vec(), false));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "会话元数据不是 UTF-8")?;
    if text.trim().is_empty() {
        return Ok((bytes.to_vec(), false));
    }
    let mut row: Value = serde_json::from_str(text).map_err(|_| {
        format!(
            "{} 含有不完整 JSON，请先关闭客户端并修复该文件",
            relative.display()
        )
    })?;
    if !row.is_object() {
        return Err("会话记录根节点不是对象".to_owned());
    }
    if kind == SessionRelocationFileKind::Document {
        if let Some(original) = row.get_mut("originalPath") {
            if original.as_str() != Some(&paths.from) {
                return Err("会话索引属于其他项目".to_owned());
            }
            *original = paths.to.clone().into();
        }
        let source = recorded(&paths.source);
        let destination = recorded(&paths.destination);
        if let Some(entries) = row.get_mut("entries") {
            for entry in entries.as_array_mut().ok_or("会话索引 entries 不是列表")? {
                for (key, from, to) in [
                    ("projectPath", paths.from.as_str(), paths.to.as_str()),
                    ("fullPath", source.as_str(), destination.as_str()),
                ] {
                    if let Some(value) = entry.get_mut(key) {
                        let old = value.as_str().ok_or("会话索引路径不是字符串")?;
                        *value = remap(old, from, to)
                            .ok_or("会话索引指向当前项目以外的位置")?
                            .into();
                    }
                }
            }
        }
        return Ok((
            rewrite_json_document(text, &row, false)?.into_bytes(),
            false,
        ));
    }
    let Some(origin) = row.get("cwd") else {
        return Ok((bytes.to_vec(), false));
    };
    let origin = origin.as_str().ok_or("会话 cwd 不是字符串")?;
    let Some(next) = remap(origin, &paths.from, &paths.to) else {
        if kind == SessionRelocationFileKind::MainTranscript && !saw_origin {
            return Err(format!("{} 的起始目录不属于所选项目", relative.display()));
        }
        return Ok((bytes.to_vec(), false));
    };
    Ok((
        rewrite_json_string_fields(text, &[], &[("cwd", next.as_str())])?.into_bytes(),
        true,
    ))
}
fn recorded(path: &Path) -> String {
    crate::services::wsl::split_host_path(path)
        .map(|(_, guest)| guest)
        .unwrap_or_else(|| {
            crate::platform::paths::compatible_path(path)
                .to_string_lossy()
                .into_owned()
        })
}
