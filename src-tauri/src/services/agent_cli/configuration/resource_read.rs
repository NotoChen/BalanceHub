//! Verified resource reads shared by content browsing and native editing.
use super::{
    native_support,
    resource_selection::{add_string_fact, select},
    selection::{rejected, value_at, DocumentSelection},
    sources, MAX_DOCUMENT_BYTES,
};
use crate::{
    models::*,
    services::agent_cli::{
        catalog::{projection::definition_anchor, ReadControl},
        environment::{
            mutation::{GuardedFile, MutationInventory},
            stable_id,
            verified_path::reopen_verified_path,
        },
    },
};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) struct ResourceDocument {
    pub key: String,
    pub label: String,
    pub source: AgentAssetSource,
    pub file: GuardedFile,
    pub text: String,
    pub format: AgentConfigurationFormat,
    pub selection: DocumentSelection,
    pub read_only_reason: Option<String>,
}

pub(super) struct ResourceRead {
    pub resource: AgentResourceContent,
    pub documents: Vec<ResourceDocument>,
    pub diagnostics: Vec<AgentConfigurationDiagnostic>,
    pub complete: bool,
}

/// One read transaction can visit several bindings of the same physical file.
/// Retain bounded bytes in memory, while validating each source's own anchor.
#[derive(Default)]
pub(super) struct ResourceReader {
    files: BTreeMap<(PathBuf, PathBuf), ResourceFile>,
    bytes: usize,
}

struct ResourceFile {
    file: GuardedFile,
    parsed: Option<Arc<ParsedResource>>,
}

struct ParsedResource {
    format: AgentConfigurationFormat,
    root: Option<Value>,
    // Native node selection and original TOML formatting need separate trees;
    // both belong to this same bounded physical-file read.
    toml: Option<toml_edit::Document>,
}

impl ParsedResource {
    fn read(path: &Path, text: &str) -> Self {
        let format = match path.extension().and_then(|extension| extension.to_str()) {
            Some("md" | "markdown") => AgentConfigurationFormat::Markdown,
            Some("toml") => AgentConfigurationFormat::Toml,
            Some("jsonc") => AgentConfigurationFormat::Jsonc,
            _ => {
                if let Ok(root) = native_support::parse(text, AgentConfigurationFormat::Json) {
                    return Self {
                        format: AgentConfigurationFormat::Json,
                        root: Some(root),
                        toml: None,
                    };
                }
                AgentConfigurationFormat::Jsonc
            }
        };
        Self {
            format,
            root: native_support::parse(text, format).ok(),
            toml: (format == AgentConfigurationFormat::Toml)
                .then(|| text.parse().ok())
                .flatten(),
        }
    }
}

impl ResourceReader {
    fn parsed(&mut self, file: &GuardedFile, text: &str) -> Arc<ParsedResource> {
        let entry = file.verified_anchor().and_then(|anchor| {
            self.files.get_mut(&(
                anchor.allowed_root().to_owned(),
                anchor.physical_path().to_owned(),
            ))
        });
        match entry {
            Some(entry) => Arc::clone(
                entry
                    .parsed
                    .get_or_insert_with(|| Arc::new(ParsedResource::read(file.path(), text))),
            ),
            None => Arc::new(ParsedResource::read(file.path(), text)),
        }
    }
    fn remember(&mut self, key: (PathBuf, PathBuf), file: GuardedFile) -> GuardedFile {
        let bytes = file.bytes().map_or(0, <[u8]>::len);
        if self.bytes.saturating_add(bytes) > 8 * 1024 * 1024 {
            self.files.clear();
            self.bytes = 0;
        }
        if bytes <= 8 * 1024 * 1024 {
            self.bytes -= self
                .files
                .get(&key)
                .and_then(|entry| entry.file.bytes())
                .map_or(0, <[u8]>::len);
            self.bytes += bytes;
            self.files.insert(
                key,
                ResourceFile {
                    file: file.clone(),
                    parsed: None,
                },
            );
        }
        file
    }

    fn capture(
        &mut self,
        snapshot: &MutationInventory,
        source: &AgentAssetSource,
    ) -> Result<GuardedFile, AgentConfigurationError> {
        let anchor = snapshot
            .source_anchors
            .get(&source.id)
            .ok_or_else(|| rejected("资源来源已失效，请刷新后重试".to_owned()))?;
        if anchor.revision().identity != source.revision.identity {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::SourceChanged,
            ));
        }
        let key = (
            anchor.allowed_root().to_owned(),
            anchor.physical_path().to_owned(),
        );
        if let Some(file) = self
            .files
            .get(&key)
            .map(|entry| &entry.file)
            .filter(|file| {
                file.bytes()
                    .is_some_and(|bytes| anchor.matches_bytes(bytes))
            })
        {
            let guard = reopen_verified_path(anchor).map_err(|_| {
                AgentConfigurationError::new(AgentConfigurationErrorKind::SourceChanged)
            })?;
            guard.revalidate().map_err(|_| {
                AgentConfigurationError::new(AgentConfigurationErrorKind::SourceChanged)
            })?;
            let mut file = file.clone();
            file.source_id = source.id.clone();
            return Ok(file);
        }
        let file =
            GuardedFile::capture_for_read(source, &snapshot.source_anchors, MAX_DOCUMENT_BYTES)?;
        Ok(self.remember(key, file))
    }

    fn capture_path(
        &mut self,
        root: &Path,
        path: &Path,
    ) -> Result<GuardedFile, AgentConfigurationError> {
        let key = (root.to_owned(), path.to_owned());
        if let Some(file) = self
            .files
            .get(&key)
            .map(|entry| &entry.file)
            .filter(|file| {
                file.verified_anchor()
                    .is_some_and(|anchor| reopen_verified_path(anchor).is_ok())
            })
        {
            return Ok(file.clone());
        }
        let file = GuardedFile::capture_path(root, path, MAX_DOCUMENT_BYTES)?;
        Ok(self.remember(key, file))
    }
}

pub(super) fn read_binding(
    reader: &mut ResourceReader,
    snapshot: &MutationInventory,
    id: &str,
    control: &ReadControl,
) -> Result<ResourceRead, AgentConfigurationError> {
    control.check().map_err(rejected)?;
    let asset = snapshot
        .inventory
        .assets
        .iter()
        .find(|asset| asset.stable_id == id)
        .ok_or_else(|| rejected("此来源已消失，请刷新资源列表".to_owned()))?;
    read_with(reader, snapshot, asset, control)
}

pub(super) fn read(
    snapshot: &MutationInventory,
    asset: &AgentAssetRecord,
    control: &ReadControl,
) -> Result<ResourceRead, AgentConfigurationError> {
    read_with(&mut ResourceReader::default(), snapshot, asset, control)
}

fn read_with(
    reader: &mut ResourceReader,
    snapshot: &MutationInventory,
    asset: &AgentAssetRecord,
    control: &ReadControl,
) -> Result<ResourceRead, AgentConfigurationError> {
    control.check().map_err(rejected)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let source = content_source(snapshot, asset)?;
    let mut diagnostics = Vec::new();
    let files = resource_files(reader, snapshot, asset, source, &mut diagnostics)?;
    let mut result = ResourceRead {
        resource: AgentResourceContent {
            asset_id: asset.stable_id.clone(),
            category: asset.category,
            description: None,
            facts: Vec::new(),
        },
        documents: Vec::new(),
        complete: diagnostics.is_empty(),
        diagnostics,
    };
    for (index, (source, file, readonly_reference)) in files.into_iter().enumerate() {
        control.check().map_err(rejected)?;
        if Instant::now() >= deadline {
            return Err(AgentConfigurationError::new(
                AgentConfigurationErrorKind::Timeout,
            ));
        }
        let original = match file
            .bytes()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        {
            Some(text) => text,
            None => {
                result.complete = false;
                result.diagnostics.push(native_support::diagnostic(
                    "resourceUnreadableText",
                    &format!(
                        "{} 无法读取为完整 UTF-8 文本",
                        file.path()
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                    ),
                    AgentConfigurationDiagnosticSeverity::Warning,
                ));
                continue;
            }
        };
        let parsed = reader.parsed(&file, original);
        let format = parsed.format;
        let mut fallback = false;
        let mut selection = if index == 0 {
            match parsed.root.as_ref() {
                Some(root) => {
                    match select(snapshot, asset, root, format, &mut result.resource.facts) {
                        Ok(selection) => selection,
                        Err(error) => {
                            result.diagnostics.push(native_support::diagnostic(
                                "resourceWholeSource",
                                &error.message,
                                AgentConfigurationDiagnosticSeverity::Info,
                            ));
                            fallback = true;
                            DocumentSelection::Whole
                        }
                    }
                }
                None => {
                    if matches!(
                        asset.category,
                        AgentAssetCategory::Mcp | AgentAssetCategory::Hook
                    ) {
                        fallback = true;
                        result.diagnostics.push(native_support::diagnostic(
                            "resourceWholeSource",
                            "文件无法解析为原生配置，显示完整来源文件，尚不能比较资源定义",
                            AgentConfigurationDiagnosticSeverity::Info,
                        ));
                    }
                    DocumentSelection::Whole
                }
            }
        } else {
            DocumentSelection::Whole
        };
        let text = match selection.text_with_document(original, parsed.toml.as_ref()) {
            Ok(text) => text,
            Err(error) => {
                result.diagnostics.push(native_support::diagnostic(
                    "resourceWholeSource",
                    &error.message,
                    AgentConfigurationDiagnosticSeverity::Info,
                ));
                fallback = true;
                selection = DocumentSelection::Whole;
                original.to_owned()
            }
        };
        if index == 0 {
            let selected = match &selection {
                DocumentSelection::Structured { path, .. } => {
                    parsed.root.as_ref().and_then(|root| value_at(root, path))
                }
                DocumentSelection::Whole => parsed.root.as_ref(),
            };
            describe(&mut result.resource, selected, &text, format);
        }
        result.complete &= !fallback;
        let (key, label) = if fallback {
            ("source", "完整来源文件")
        } else if format == AgentConfigurationFormat::Markdown {
            if asset.category == AgentAssetCategory::Skill {
                ("body", "SKILL.md")
            } else {
                ("readme", "说明文档")
            }
        } else {
            (
                "definition",
                match asset.category {
                    AgentAssetCategory::Mcp => "MCP 配置",
                    AgentAssetCategory::Hook => "Hook 执行配置",
                    AgentAssetCategory::Plugin | AgentAssetCategory::Extension => "插件定义",
                    _ => "配置内容",
                },
            )
        };
        let read_only_reason = read_only_reason(file.path(), readonly_reference);
        result.documents.push(ResourceDocument {
            key: key.to_owned(),
            label: label.to_owned(),
            source,
            file,
            text,
            format,
            selection,
            read_only_reason,
        });
    }
    if result.documents.is_empty() {
        return Err(rejected("来源中没有可读取的 UTF-8 文本".to_owned()));
    }
    Ok(result)
}

fn read_only_reason(path: &Path, readonly_reference: bool) -> Option<String> {
    if readonly_reference {
        Some("此文件通过共享目录链接读取，请在原始目录编辑".to_owned())
    } else if !sources::file_permissions_allow_write(path, false) {
        Some("当前用户没有文件或所在目录的写入权限".to_owned())
    } else {
        None
    }
}

pub(super) fn describe(
    resource: &mut AgentResourceContent,
    root: Option<&Value>,
    original: &str,
    format: AgentConfigurationFormat,
) {
    if format == AgentConfigurationFormat::Markdown {
        let mut lines = original.lines();
        if lines.next().is_some_and(|line| line.trim() == "---") {
            let mut header = String::new();
            for line in lines.take(128) {
                if line.trim() == "---" {
                    if let Ok(value) = serde_yaml_ng::from_str::<Value>(&header) {
                        resource.description = value
                            .get("description")
                            .and_then(Value::as_str)
                            .map(str::to_owned);
                    }
                    break;
                }
                if header.len() + line.len() > 16 * 1024 {
                    break;
                }
                header.push_str(line);
                header.push('\n');
            }
        }
    } else if let Some(root) = root {
        resource.description = root
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if matches!(
            resource.category,
            AgentAssetCategory::Plugin | AgentAssetCategory::Extension
        ) {
            add_string_fact(&mut resource.facts, "版本", root.get("version"));
            add_string_fact(
                &mut resource.facts,
                "作者",
                root.get("author")
                    .and_then(|value| value.get("name"))
                    .or_else(|| root.get("author")),
            );
        }
    }
}

fn content_source<'a>(
    snapshot: &'a MutationInventory,
    asset: &AgentAssetRecord,
) -> Result<&'a AgentAssetSource, AgentConfigurationError> {
    if matches!(
        asset.category,
        AgentAssetCategory::Plugin | AgentAssetCategory::Extension
    ) {
        // Claude's Definition is its registry entry. Its already-resolved
        // manifest/root observations carry the package contents instead.
        let represented = snapshot
            .inventory
            .sources
            .iter()
            .filter(|source| {
                source.context_id == asset.context_id
                    && (asset.source_ids.contains(&source.id)
                        || snapshot.inventory.declarations.iter().any(|declaration| {
                            asset.represented_declaration_ids.contains(&declaration.id)
                                && declaration.source_id == source.id
                        }))
                    && !source.revision.is_missing
            })
            .collect::<Vec<_>>();
        for manifests in [true, false] {
            let candidates = represented
                .iter()
                .copied()
                .filter(|source| {
                    if manifests {
                        source.source_kind == AgentAssetSourceKind::File
                            && matches!(
                                Path::new(&source.path)
                                    .file_name()
                                    .and_then(|name| name.to_str()),
                                Some("plugin.json" | "extension.json" | "gemini-extension.json")
                            )
                    } else {
                        source.source_kind == AgentAssetSourceKind::Directory
                    }
                })
                .collect::<Vec<_>>();
            match candidates.as_slice() {
                [source] => return Ok(source),
                [] => {}
                _ => {
                    return Err(rejected(
                        "插件存在多份来源，暂时无法确认要读取的定义文件".to_owned(),
                    ))
                }
            }
        }
        return Err(rejected(
            "未找到插件的可读定义目录或说明文件，请检查原生安装状态".to_owned(),
        ));
    }
    let source_id = definition_anchor(snapshot, asset)
        .map_or(&asset.inspection_source_id, |declaration| {
            &declaration.source_id
        });
    snapshot
        .inventory
        .sources
        .iter()
        .find(|source| &source.id == source_id)
        .ok_or_else(|| rejected("没有可读取的资源文件".to_owned()))
}

fn resource_files(
    reader: &mut ResourceReader,
    snapshot: &MutationInventory,
    asset: &AgentAssetRecord,
    source: &AgentAssetSource,
    diagnostics: &mut Vec<AgentConfigurationDiagnostic>,
) -> Result<Vec<(AgentAssetSource, GuardedFile, bool)>, AgentConfigurationError> {
    let anchor = snapshot
        .source_anchors
        .get(&source.id)
        .ok_or_else(|| rejected("资源来源已失效，请刷新后重试".to_owned()))?;
    let guard = reopen_verified_path(anchor)
        .map_err(|_| AgentConfigurationError::new(AgentConfigurationErrorKind::SourceChanged))?;
    let readonly = anchor.is_readonly_reference();
    let mut files = Vec::new();
    if source.source_kind == AgentAssetSourceKind::File {
        files.push((source.clone(), reader.capture(snapshot, source)?, readonly));
    } else {
        let candidates: &[&str] = if asset.category == AgentAssetCategory::Skill {
            &["SKILL.md"]
        } else {
            &[
                ".agent-plugin/plugin.json",
                ".codex-plugin/plugin.json",
                ".claude-plugin/plugin.json",
                ".grok-plugin/plugin.json",
                "gemini-extension.json",
                "plugin.json",
                "extension.json",
            ]
        };
        for relative in candidates {
            let path = Path::new(&source.path).join(relative);
            if candidate_is_file(&path, relative, diagnostics) {
                files.push(child_file(reader, source, &path)?);
                break;
            }
        }
    }
    if matches!(
        asset.category,
        AgentAssetCategory::Plugin | AgentAssetCategory::Extension
    ) {
        let root = if let Some((_, file, _)) = files.first() {
            let parent = file
                .path()
                .parent()
                .ok_or_else(|| rejected("插件目录无效".to_owned()))?;
            if parent.file_name().is_some_and(|name| {
                matches!(
                    name.to_str(),
                    Some(
                        ".agent-plugin"
                            | ".codex-plugin"
                            | ".claude-plugin"
                            | ".grok-plugin"
                            | ".cursor-plugin"
                    )
                )
            }) {
                parent.parent().unwrap_or(parent)
            } else {
                parent
            }
        } else {
            Path::new(&source.path)
        };
        for name in ["README.md", "readme.md", "README.zh-CN.md", "README_CN.md"] {
            let path = root.join(name);
            if candidate_is_file(&path, name, diagnostics) {
                match child_file(reader, source, &path) {
                    Ok(file) => files.push(file),
                    Err(error) => diagnostics.push(native_support::diagnostic(
                        "resourceUnreadableReadme",
                        &format!("{name} 无法读取：{}", error.message),
                        AgentConfigurationDiagnosticSeverity::Warning,
                    )),
                }
                break;
            }
        }
    }
    if files.is_empty() {
        return Err(rejected(
            "资源目录中没有找到可读取的定义文件或 README".to_owned(),
        ));
    }
    if readonly {
        for (_, _, readonly_reference) in &mut files {
            *readonly_reference = true;
        }
    }
    guard
        .revalidate()
        .map_err(|_| AgentConfigurationError::new(AgentConfigurationErrorKind::SourceChanged))?;
    Ok(files)
}

fn candidate_is_file(
    path: &Path,
    label: &str,
    diagnostics: &mut Vec<AgentConfigurationDiagnostic>,
) -> bool {
    let reason = match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => return true,
        Ok(_) => "不是普通文件".to_owned(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(error) => format!("无法检查文件：{error}"),
    };
    diagnostics.push(native_support::diagnostic(
        "resourceUnreadableFile",
        &format!("{label} {reason}"),
        AgentConfigurationDiagnosticSeverity::Warning,
    ));
    false
}

fn child_file(
    reader: &mut ResourceReader,
    source: &AgentAssetSource,
    path: &Path,
) -> Result<(AgentAssetSource, GuardedFile, bool), AgentConfigurationError> {
    let mut file = reader.capture_path(Path::new(&source.allowed_root), path)?;
    let mut child = source.clone();
    child.id = stable_id(
        "resource-document",
        &[&source.context_id, &path.to_string_lossy()],
    );
    child.path = path.to_string_lossy().into_owned();
    child.source_kind = AgentAssetSourceKind::File;
    child.revision = file.revision();
    file.source_id = child.id.clone();
    Ok((child, file, false))
}
