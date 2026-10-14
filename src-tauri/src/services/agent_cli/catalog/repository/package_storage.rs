//! Versioned library metadata with immutable, content-addressed Skill files.
//! Old inline storage is read only for the one-way migration on the next save.
use super::{Library, LIBRARY_LIMIT, MAX_LIBRARY_ENTRIES};
use crate::services::agent_cli::{
    catalog::{
        definition::{DefinitionPayload, PackageFile, StoredDefinition},
        package,
    },
    config_support::{directories, file_writer::StagedFile, native_history},
    environment::mutation::GuardedFile,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, sync::Arc};

const CONTENT_BUDGET: usize = 256 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
struct Document {
    format_version: u8,
    library: Library,
    definitions: BTreeMap<String, Vec<Version>>,
}
#[derive(Serialize, Deserialize)]
struct Version {
    version: u64,
    payload: Payload,
}
#[derive(Serialize, Deserialize)]
enum Payload {
    Native(DefinitionPayload),
    Skill(BTreeMap<String, Blob>),
}
#[derive(Serialize, Deserialize)]
struct Blob {
    hash: String,
    bytes: usize,
    executable: bool,
}

pub(super) fn encode(root: &Path, library: &Library, save: bool) -> Result<Vec<u8>, String> {
    if library.entries.len() > MAX_LIBRARY_ENTRIES {
        return Err("共享库超过 4096 项".to_owned());
    }
    let mut metadata = library.clone();
    let mut definitions = BTreeMap::new();
    let mut by_pointer = BTreeMap::new();
    let mut blobs = BTreeMap::<String, Arc<[u8]>>::new();
    let mut total = 0usize;
    for (id, entry) in &mut metadata.entries {
        let mut versions = Vec::new();
        for definition in std::mem::take(&mut entry.versions) {
            let payload = match definition.payload {
                DefinitionPayload::Skill(files) => {
                    let mut refs = BTreeMap::new();
                    let mut package_bytes = 0usize;
                    if files.len() > package::MAX_PACKAGE_FILES {
                        return Err("Skill 文件数量超过 8192 项".to_owned());
                    }
                    for (path, file) in files {
                        package::validate_relative(&path)?;
                        package_bytes = package_bytes.saturating_add(file.bytes.len());
                        if file.bytes.len() > package::MAX_FILE_BYTES
                            || package_bytes > package::MAX_PACKAGE_BYTES
                        {
                            return Err("Skill 超过文件或整包预算".to_owned());
                        }
                        let pointer = (file.bytes.as_ptr() as usize, file.bytes.len());
                        let hash = by_pointer
                            .entry(pointer)
                            .or_insert_with(|| native_history::hash(&file.bytes))
                            .clone();
                        if !blobs.contains_key(&hash) {
                            total = total.saturating_add(file.bytes.len());
                            if total > CONTENT_BUDGET {
                                return Err(
                                    "共享库去重后的内容超过 256 MiB，请先整理不再使用的版本"
                                        .to_owned(),
                                );
                            }
                            blobs.insert(hash.clone(), Arc::clone(&file.bytes));
                        }
                        refs.insert(
                            path,
                            Blob {
                                hash,
                                bytes: file.bytes.len(),
                                executable: file.executable,
                            },
                        );
                    }
                    Payload::Skill(refs)
                }
                payload => Payload::Native(payload),
            };
            versions.push(Version {
                version: definition.version,
                payload,
            });
        }
        definitions.insert(id.clone(), versions);
    }
    let bytes = serde_json::to_vec(&Document {
        format_version: 2,
        library: metadata,
        definitions,
    })
    .map_err(|_| "共享库元数据编码失败")?;
    if bytes.len() > LIBRARY_LIMIT {
        return Err("共享库元数据超过 64 MiB".to_owned());
    }
    if save && !blobs.is_empty() {
        let directory = root.join("skill-blobs");
        directories::ensure_directory(root, &directory)?;
        for (hash, bytes) in blobs {
            let source =
                GuardedFile::capture_path(root, &directory.join(&hash), package::MAX_FILE_BYTES)
                    .map_err(|_| "共享文件存储不可访问")?;
            if let Some(existing) = source.bytes() {
                if existing != bytes.as_ref() {
                    return Err("共享文件内容校验失败，未修改已有版本".to_owned());
                }
                continue;
            }
            let mut stage =
                StagedFile::prepare(&source, &bytes, || Ok(())).map_err(|error| error.message)?;
            stage.commit(|| Ok(())).map_err(|error| error.message)?;
            stage.verify().map_err(|error| error.message)?;
        }
    }
    Ok(bytes)
}

pub(super) fn decode(root: &Path, bytes: Option<&[u8]>) -> Result<Library, String> {
    let Some(bytes) = bytes else {
        return Ok(Library::default());
    };
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "共享库损坏，已保留原文件")?;
    if value.get("format_version").is_none() {
        return serde_json::from_value(value)
            .map_err(|_| "共享库迁移前读取失败，未覆盖原文件".to_owned());
    }
    let document: Document = serde_json::from_value(value).map_err(|_| "共享库元数据无效")?;
    if document.format_version != 2 {
        return Err("共享库格式版本不受支持".to_owned());
    }
    let mut library = document.library;
    let mut loaded = BTreeMap::<String, Arc<[u8]>>::new();
    let mut total = 0usize;
    for (id, versions) in document.definitions {
        let entry = library.entries.get_mut(&id).ok_or("共享版本缺少资产记录")?;
        if !entry.versions.is_empty() {
            return Err("共享库重复保存了版本".to_owned());
        }
        for version in versions {
            let payload = match version.payload {
                Payload::Native(DefinitionPayload::Skill(_)) => {
                    return Err("Skill 版本必须使用独立文件存储".to_owned())
                }
                Payload::Native(value) => value,
                Payload::Skill(refs) => {
                    if refs.len() > package::MAX_PACKAGE_FILES {
                        return Err("共享 Skill 文件过多".to_owned());
                    }
                    let mut files = BTreeMap::new();
                    let mut package_bytes = 0usize;
                    for (path, blob) in refs {
                        package::validate_relative(&path)?;
                        if !native_history::valid_id(&blob.hash)
                            || blob.bytes > package::MAX_FILE_BYTES
                        {
                            return Err("共享文件引用无效".to_owned());
                        }
                        let bytes = if let Some(bytes) = loaded.get(&blob.hash) {
                            Arc::clone(bytes)
                        } else {
                            let source = GuardedFile::capture_path(
                                root,
                                &root.join("skill-blobs").join(&blob.hash),
                                package::MAX_FILE_BYTES,
                            )
                            .map_err(|_| "共享版本的文件无法读取")?;
                            let bytes = source.bytes().ok_or("共享版本的文件已丢失")?;
                            if bytes.len() != blob.bytes || native_history::hash(bytes) != blob.hash
                            {
                                return Err("共享版本的文件校验失败".to_owned());
                            }
                            total = total.saturating_add(bytes.len());
                            if total > CONTENT_BUDGET {
                                return Err("共享库内容超过读取预算".to_owned());
                            }
                            let bytes: Arc<[u8]> = bytes.into();
                            loaded.insert(blob.hash.clone(), Arc::clone(&bytes));
                            bytes
                        };
                        if bytes.len() != blob.bytes {
                            return Err("共享文件长度记录不一致".to_owned());
                        }
                        package_bytes = package_bytes.saturating_add(bytes.len());
                        if package_bytes > package::MAX_PACKAGE_BYTES {
                            return Err("共享 Skill 超过 64 MiB".to_owned());
                        }
                        files.insert(
                            path,
                            PackageFile {
                                bytes,
                                executable: blob.executable,
                            },
                        );
                    }
                    DefinitionPayload::Skill(files)
                }
            };
            entry.versions.push(StoredDefinition {
                version: version.version,
                payload,
            });
        }
    }
    Ok(library)
}
