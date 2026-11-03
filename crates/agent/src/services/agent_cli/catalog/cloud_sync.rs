use super::{
    definition::{DefinitionPayload, PackageFile},
    repository::{Entry, Library},
    CatalogService,
};
use crate::{
    models::AgentAssetCategory,
    services::cloud_sync::{SyncDocument, SyncDocuments},
};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

fn export(library: &Library) -> Result<SyncDocuments, String> {
    let mut documents = BTreeMap::new();
    for (id, scheme) in &library.instructions {
        documents.insert(
            format!("instruction/{id}"),
            SyncDocument {
                title: scheme.name.clone(),
                category: "共享指令".to_owned(),
                value: serde_json::to_value(scheme).map_err(|_| "共享指令格式无效")?,
            },
        );
    }
    for (id, entry) in &library.entries {
        let Some(definition) = entry.current() else {
            continue;
        };
        let mut value = match &definition.payload {
            DefinitionPayload::Skill(files) => {
                let mut references = serde_json::Map::new();
                for (path, file) in files {
                    let key = format!("blob/{:x}", Sha256::digest(&file.bytes));
                    references.insert(
                        path.clone(),
                        json!({ "blob": key, "executable": file.executable }),
                    );
                    documents.entry(key).or_insert_with(|| SyncDocument {
                        title: "资源文件".to_owned(),
                        category: "blob".to_owned(),
                        value: json!({ "content": STANDARD.encode(&file.bytes) }),
                    });
                }
                json!({ "name": entry.name, "category": entry.category, "files": references })
            }
            payload => {
                json!({ "name": entry.name, "category": entry.category, "definition": payload })
            }
        };
        if let Some(origin) = &entry.origin {
            value["origin"] = serde_json::to_value(origin).map_err(|_| "Skill 来源格式无效")?;
        }
        documents.insert(
            format!("asset/{id}"),
            SyncDocument {
                title: entry.name.clone(),
                category: "共享资产".to_owned(),
                value,
            },
        );
    }
    Ok(documents)
}

fn import(library: &mut Library, documents: &SyncDocuments) -> Result<(), String> {
    let mut instructions = BTreeMap::new();
    for (key, document) in documents {
        if let Some(id) = key.strip_prefix("instruction/") {
            let scheme: crate::models::AgentInstructionSet =
                serde_json::from_value(document.value.clone()).map_err(|_| "共享指令格式无效")?;
            super::instructions::validate(&scheme)?;
            if id != scheme.id || scheme.revision != super::instructions::revision(&scheme) {
                return Err("共享指令身份或内容校验失败".to_owned());
            }
            instructions.insert(id.to_owned(), scheme);
        }
    }
    if instructions.len() > super::instructions::MAX_SETS {
        return Err("共享指令方案超过 128 个".to_owned());
    }
    library.instructions = instructions;
    let mut incoming = BTreeMap::new();
    for (key, document) in documents {
        let Some(id) = key.strip_prefix("asset/") else {
            continue;
        };
        if id.is_empty() || id.len() > 256 || id.contains('/') {
            return Err("共享资产标识无效".to_owned());
        }
        let name = document
            .value
            .get("name")
            .and_then(|value| value.as_str())
            .ok_or("共享资产名称无效")?;
        let category: AgentAssetCategory = serde_json::from_value(
            document
                .value
                .get("category")
                .cloned()
                .ok_or("共享资产类型缺失")?,
        )
        .map_err(|_| "共享资产类型无效")?;
        if category == AgentAssetCategory::Hook {
            super::definition::validate_hook_display_name(name)?;
        } else {
            super::definition::validate_name(name)?;
        }
        let payload = if category == AgentAssetCategory::Skill {
            let files = document
                .value
                .get("files")
                .and_then(|value| value.as_object())
                .ok_or("Skill 文件清单无效")?;
            let mut package = BTreeMap::new();
            let mut size = 0usize;
            for (path, file) in files {
                super::package::validate_relative(path)?;
                let blob = file
                    .get("blob")
                    .and_then(|value| value.as_str())
                    .ok_or("Skill 文件引用无效")?;
                let content = documents
                    .get(blob)
                    .and_then(|doc| doc.value.get("content"))
                    .and_then(|value| value.as_str())
                    .ok_or("Skill 文件缺失")?;
                let bytes = STANDARD.decode(content).map_err(|_| "Skill 文件编码无效")?;
                if blob != format!("blob/{:x}", Sha256::digest(&bytes)) {
                    return Err("Skill 文件内容校验失败".to_owned());
                }
                size = size.saturating_add(bytes.len());
                if bytes.len() > super::package::MAX_FILE_BYTES
                    || size > super::package::MAX_PACKAGE_BYTES
                    || files.len() > super::package::MAX_PACKAGE_FILES
                {
                    return Err("云端 Skill 超过共享库支持的大小，未应用同步数据".to_owned());
                }
                let executable = file
                    .get("executable")
                    .and_then(|value| value.as_bool())
                    .ok_or("Skill 文件权限标记无效")?;
                package.insert(
                    path.clone(),
                    PackageFile {
                        bytes: bytes.into(),
                        executable,
                    },
                );
            }
            let manifest = package.get("SKILL.md").ok_or("云端 Skill 缺少 SKILL.md")?;
            super::definition::validate_skill(&manifest.bytes)?;
            DefinitionPayload::Skill(package)
        } else {
            let payload: DefinitionPayload = serde_json::from_value(
                document
                    .value
                    .get("definition")
                    .cloned()
                    .ok_or("共享定义缺失")?,
            )
            .map_err(|_| "共享定义格式无效")?;
            match (&payload, category) {
                (DefinitionPayload::Mcp(definition), AgentAssetCategory::Mcp) => {
                    super::definition::validate_mcp(definition)?
                }
                (DefinitionPayload::Hook(values), AgentAssetCategory::Hook) => {
                    for (kind, value) in values {
                        super::hook_definition::validate(*kind, value)?;
                    }
                }
                _ => return Err("共享定义与资产类型不一致".to_owned()),
            }
            payload
        };
        let origin = document
            .value
            .get("origin")
            .map(|value| {
                if category != AgentAssetCategory::Skill {
                    return Err("只有 Skill 可以关联来源仓库".to_owned());
                }
                let origin =
                    serde_json::from_value(value.clone()).map_err(|_| "Skill 来源格式无效")?;
                super::upstream::normalize(origin)
            })
            .transpose()?;
        incoming.insert(id.to_owned(), (name.to_owned(), category, payload, origin));
    }
    for (id, entry) in &mut library.entries {
        if entry.current().is_some() && !incoming.contains_key(id) {
            entry.shared_deleted = true;
        }
    }
    for (id, (name, category, payload, origin)) in incoming {
        let entry = library
            .entries
            .entry(id)
            .or_insert_with(|| Entry::new(name.clone(), category));
        if entry.category != category {
            return Err("共享资产类型发生冲突，未覆盖原定义".to_owned());
        }
        entry.set_definition(&name, payload)?;
        entry.origin = origin;
    }
    Ok(())
}

pub(crate) fn only_catalog(documents: &SyncDocuments) -> SyncDocuments {
    documents
        .iter()
        .filter(|(key, _)| {
            key.starts_with("asset/") || key.starts_with("blob/") || key.starts_with("instruction/")
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

impl CatalogService {
    pub(crate) fn validate_cloud_snapshot(&self, documents: &SyncDocuments) -> Result<(), String> {
        self.repository.read_view(|library| {
            let mut next = library.clone();
            import(&mut next, documents)?;
            if export(&next)? != only_catalog(documents) {
                return Err("云端共享定义不能无损应用，请核对资源格式与应用版本".to_owned());
            }
            self.repository.validate_capacity(&next)?;
            Ok(())
        })
    }
    pub(crate) fn cloud_snapshot(&self) -> Result<SyncDocuments, String> {
        self.repository.read_view(export)
    }

    /// The application transaction owns its mutation gate before entering this
    /// library transaction. The callback journals both files before publishing.
    pub(crate) fn apply_cloud_snapshot<T>(
        &self,
        expected: &SyncDocuments,
        desired: &SyncDocuments,
        commit: impl FnOnce(
            Option<&[u8]>,
            Option<&[u8]>,
            &mut dyn FnMut() -> Result<(), String>,
        ) -> Result<T, String>,
    ) -> Result<T, String> {
        let expected = only_catalog(expected);
        let desired = only_catalog(desired);
        if expected == desired {
            return self.repository.read_view(|library| {
                if export(library)? != expected {
                    return Err("共享库在同步期间已变化，请重新同步".to_owned());
                }
                commit(None, None, &mut || Ok(()))
            });
        }
        let operations = self
            .operations
            .lock()
            .map_err(|_| "资产后台任务状态不可用")?;
        let ids = expected
            .keys()
            .chain(desired.keys())
            .filter_map(|key| key.strip_prefix("asset/"))
            .map(str::to_owned)
            .collect();
        if operations
            .values()
            .any(|operation| operation.overlaps(&ids, &Default::default(), &[]))
        {
            return Err("共享资产正在执行本机任务，同步将在任务完成后重试".to_owned());
        }
        let output = self
            .repository
            .checkpointed_with_original(|library, original, checkpoint| {
                if export(library)? != expected {
                    return Err("共享库在同步期间已变化，请重新同步".to_owned());
                }
                import(library, &desired)?;
                // The recovery journal must contain exactly the bytes that the
                // repository checkpoint publishes, including blob references.
                let after_bytes = self.repository.snapshot_bytes(library)?;
                commit(original, Some(&after_bytes), &mut || checkpoint(library))
            });
        self.clear_unpersisted_observations();
        output
    }
}
