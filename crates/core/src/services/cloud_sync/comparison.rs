use super::{core::Prepared, format::SyncDocuments};
use crate::models::{CloudSyncComparison, CloudSyncFileComparison};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::BTreeSet;

pub(super) fn compare(prepared: &Prepared, key: &str) -> Result<CloudSyncComparison, String> {
    let review = prepared.review()?;
    let change = review
        .changes
        .iter()
        .find(|change| change.key == key)
        .ok_or("该条目已不在同步差异中")?;
    let left = prepared.local.get(key);
    let right = prepared.remote.get(key);
    let mut files = Vec::new();
    let paths: BTreeSet<_> = left
        .into_iter()
        .chain(right)
        .filter_map(|document| {
            document
                .value
                .get("files")
                .and_then(|value| value.as_object())
        })
        .flat_map(|files| files.keys())
        .collect();
    if !paths.is_empty() {
        for path in paths {
            let local = file_content(&prepared.local, key, path)?;
            let remote = file_content(&prepared.remote, key, path)?;
            if local == remote {
                continue;
            }
            let binary = local.as_ref().is_some_and(|(bytes, _)| is_binary(bytes))
                || remote.as_ref().is_some_and(|(bytes, _)| is_binary(bytes));
            files.push(CloudSyncFileComparison {
                path: path.to_owned(),
                local_text: render_file(local, binary),
                remote_text: render_file(remote, binary),
                binary,
            });
        }
    }
    // Keep renames, executable bits and non-file definitions visible too.
    let local_metadata = metadata(left)?;
    let remote_metadata = metadata(right)?;
    if local_metadata != remote_metadata || files.is_empty() {
        files.insert(
            0,
            CloudSyncFileComparison {
                path: "配置".to_owned(),
                local_text: local_metadata,
                remote_text: remote_metadata,
                binary: false,
            },
        );
    }
    Ok(CloudSyncComparison {
        title: change.title.clone(),
        files,
    })
}

fn metadata(document: Option<&super::format::SyncDocument>) -> Result<String, String> {
    let Some(document) = document else {
        return Ok(String::new());
    };
    let mut value = document.value.clone();
    if let Some(files) = value
        .get_mut("files")
        .and_then(|value| value.as_object_mut())
    {
        for file in files.values_mut() {
            if let Some(object) = file.as_object_mut() {
                object.remove("blob");
            }
        }
    }
    serde_json::to_string_pretty(&value).map_err(|_| "无法生成配置差异".to_owned())
}

fn file_content(
    documents: &SyncDocuments,
    key: &str,
    path: &str,
) -> Result<Option<(Vec<u8>, bool)>, String> {
    let Some(file) = documents
        .get(key)
        .and_then(|doc| doc.value.get("files"))
        .and_then(|files| files.get(path))
    else {
        return Ok(None);
    };
    let blob = file
        .get("blob")
        .and_then(|value| value.as_str())
        .ok_or("资源引用无效")?;
    let content = documents
        .get(blob)
        .and_then(|doc| doc.value.get("content"))
        .and_then(|value| value.as_str())
        .ok_or("资源文件缺失")?;
    let bytes = STANDARD.decode(content).map_err(|_| "资源文件编码无效")?;
    Ok(Some((
        bytes,
        file.get("executable")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
    )))
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0) || std::str::from_utf8(bytes).is_err()
}

fn render_file(file: Option<(Vec<u8>, bool)>, binary: bool) -> String {
    let Some((bytes, executable)) = file else {
        return String::new();
    };
    if binary {
        format!(
            "二进制文件：{} 字节\nSHA-256：{}\n可执行：{}",
            bytes.len(),
            super::format::digest(&bytes),
            executable
        )
    } else {
        String::from_utf8_lossy(&bytes).into_owned()
    }
}
