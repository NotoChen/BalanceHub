//! Read one resource across its sources; identities and comparisons stay here.
use super::{
    native_support, resource_read,
    resource_selection::{add_string_fact, hook_facts},
    selection::DocumentSelection,
};
use crate::{
    models::*,
    services::agent_cli::{
        catalog::{CatalogService, PublishedCatalog, ReadControl},
        environment::mutation::MutationInventory,
    },
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, VecDeque},
    time::{Duration, Instant},
};

const MAX_CONTENT_BYTES: usize = 8 * 1024 * 1024;

struct ContentDraft {
    description: Option<String>,
    facts: Vec<AgentResourceContentFact>,
    documents: Vec<AgentCatalogContentDocument>,
    source: AgentCatalogContentSource,
    complete: bool,
    notes: Vec<String>,
}

pub(crate) fn read_catalog_content(
    catalog: &CatalogService,
    asset_id: &str,
    publication: &PublishedCatalog,
    control: &ReadControl,
    mut refresh: impl FnMut(AgentCliKind) -> Result<MutationInventory, String>,
    progress: impl Fn(&AgentCatalogContent) -> Result<(), String>,
) -> Result<AgentCatalogContent, String> {
    control.check()?;
    let sources = catalog.content_sources(asset_id, publication)?;
    let asset = sources.asset;
    let shared = sources.shared;
    let mut result = AgentCatalogContent {
        asset_id: asset_id.to_owned(),
        category: asset.category,
        shared_version: shared.as_ref().map(|definition| definition.version),
        groups: Vec::new(),
        unavailable: Vec::new(),
        pending_sources: asset.bindings.len(),
    };
    let mut reader = resource_read::ResourceReader::default();
    let mut refreshed = BTreeSet::new();
    let mut last_progress = Instant::now();
    let mut shown = false;
    let mut remaining = MAX_CONTENT_BYTES;
    if let Some(definition) = shared {
        match shared_content(definition, sources.mcp_connection) {
            Ok(drafts) => {
                for draft in drafts {
                    add_group(&mut result, draft, &mut remaining)?;
                }
            }
            Err(reason) => result.unavailable.push(AgentCatalogContentUnavailable {
                source: shared_source(None),
                reason,
            }),
        }
    }
    if !result.groups.is_empty() && result.pending_sources > 0 {
        finish_content(&mut result);
        progress(&result)?;
        shown = true;
    }
    // A changed source's fallback scan must not delay other readable sources.
    let mut bindings = asset
        .bindings
        .iter()
        .map(|binding| {
            (
                binding,
                publication.needs_agent_refresh(binding.native.agent_kind),
            )
        })
        .collect::<VecDeque<_>>();
    while let Some((binding, retry)) = bindings.pop_front() {
        control.check()?;
        let mut source = AgentCatalogContentSource {
            binding_id: Some(binding.id.clone()),
            agent_kind: Some(binding.native.agent_kind),
            scope: Some(binding.native.scope),
            label: asset
                .hook_source(&binding.id)
                .map(|source| source.label.clone()),
            path: binding.native.path.clone(),
            files: Vec::new(),
        };
        let kind = binding.native.agent_kind;
        let refresh_error = if retry && refreshed.insert(kind) {
            match refresh(kind) {
                Ok(current) => {
                    control.check()?;
                    publication.remember_agent(kind, current);
                    reader = resource_read::ResourceReader::default();
                    None
                }
                Err(reason) => Some(super::selection::rejected(reason)),
            }
        } else {
            None
        };
        let snapshot = publication.snapshot_for(kind);
        let read = match refresh_error {
            Some(error) => Err(error),
            None => resource_read::read_binding(&mut reader, &snapshot, &binding.id, control),
        };
        if !retry
            && read.as_ref().is_err_and(|error| {
                matches!(
                    error.kind,
                    AgentConfigurationErrorKind::SourceChanged
                        | AgentConfigurationErrorKind::RootChanged
                )
            })
        {
            bindings.push_back((binding, true));
            continue;
        }
        result.pending_sources -= 1;
        control.check()?;
        match read {
            Ok(mut content) => {
                let mut documents = Vec::new();
                for mut document in content.documents {
                    let selected_path = match &document.selection {
                        DocumentSelection::Whole => &[][..],
                        DocumentSelection::Structured { path, .. } => path.as_slice(),
                    };
                    let target =
                        serde_json::to_vec(&(document.file.path(), selected_path, document.format))
                            .map_err(|_| "无法确认编辑目标")?;
                    source.files.push(AgentCatalogContentFile {
                        key: document.key.clone(),
                        document_id: Some(document.source.id),
                        target_id: format!("resource-target-{:x}", Sha256::digest(target)),
                        path: Some(document.file.path().to_string_lossy().into_owned()),
                        read_only_reason: document.read_only_reason,
                    });
                    if asset.category == AgentAssetCategory::Mcp && content.complete {
                        let normalized = native_support::parse(&document.text, document.format)
                            .map_err(|error| error.message)
                            .and_then(|root| {
                                crate::services::agent_cli::definition(kind)
                                    .environment
                                    .catalog_adapter()
                                    .ok_or_else(|| "此 Agent 没有 MCP 连接解析器".to_owned())?
                                    .decode(&root)
                            })
                            .map(|definition| definition.connection_document());
                        match normalized {
                            Ok(connection) => {
                                document.text = serde_json::to_string_pretty(&connection)
                                    .map_err(|_| "MCP 连接配置无法展示")?;
                                document.format = AgentConfigurationFormat::Json;
                                document.label = "MCP 连接配置".to_owned();
                                content.resource.description = None;
                                content.resource.facts = mcp_facts(&connection);
                            }
                            Err(reason) => {
                                content.complete = false;
                                content.diagnostics.push(native_support::diagnostic(
                                    "mcpConnectionUnavailable",
                                    &reason,
                                    AgentConfigurationDiagnosticSeverity::Warning,
                                ));
                            }
                        }
                    }
                    documents.push(AgentCatalogContentDocument {
                        key: document.key,
                        label: document.label,
                        format: document.format,
                        text: document.text,
                    });
                }
                // Explanatory Markdown precedes configuration files for plugins.
                documents.sort_by_key(|document| {
                    (
                        document.format != AgentConfigurationFormat::Markdown,
                        document.key.clone(),
                    )
                });
                add_group(
                    &mut result,
                    ContentDraft {
                        description: content.resource.description,
                        facts: content.resource.facts,
                        documents,
                        source,
                        complete: content.complete,
                        notes: content
                            .diagnostics
                            .into_iter()
                            .map(|diagnostic| diagnostic.message)
                            .collect(),
                    },
                    &mut remaining,
                )?;
            }
            Err(error) => result.unavailable.push(AgentCatalogContentUnavailable {
                source,
                reason: error.message,
            }),
        }
        if !result.groups.is_empty()
            && result.pending_sources > 0
            && (!shown || last_progress.elapsed() >= Duration::from_millis(120))
        {
            finish_content(&mut result);
            progress(&result)?;
            last_progress = Instant::now();
            shown = true;
        }
    }
    for target in &asset.unresolved_targets {
        result.unavailable.push(AgentCatalogContentUnavailable {
            source: AgentCatalogContentSource {
                binding_id: Some(target.target_id.clone()),
                agent_kind: Some(target.agent_kind),
                scope: Some(target.scope),
                label: asset
                    .hook_source(&target.target_id)
                    .map(|source| source.label.clone()),
                path: None,
                files: Vec::new(),
            },
            reason: target.message.clone(),
        });
    }
    control.check()?;
    finish_content(&mut result);
    Ok(result)
}

fn finish_content(result: &mut AgentCatalogContent) {
    for group in &mut result.groups {
        group.comparison = None;
    }
    result.groups.sort_by_key(|group| {
        (
            !group.complete,
            !group
                .sources
                .iter()
                .any(|source| source.binding_id.is_none()),
        )
    });
    if let Some((primary, variants)) = result.groups.split_first_mut() {
        for variant in variants {
            variant.comparison = Some(compare(primary, variant));
        }
    }
}

fn add_group(
    result: &mut AgentCatalogContent,
    draft: ContentDraft,
    remaining: &mut usize,
) -> Result<(), String> {
    if let Some(group) = result.groups.iter_mut().find(|group| {
        group.complete
            && draft.complete
            && group.description == draft.description
            && group.facts == draft.facts
            && group.documents == draft.documents
    }) {
        group.sources.push(draft.source);
        return Ok(());
    }
    let bytes = draft
        .documents
        .iter()
        .map(|document| document.text.len())
        .sum::<usize>();
    if bytes > *remaining {
        result.unavailable.push(AgentCatalogContentUnavailable {
            source: draft.source,
            reason: "内容总量超过 8 MiB，此来源尚未参与比较".to_owned(),
        });
        return Ok(());
    }
    *remaining -= bytes;
    let identity = serde_json::to_vec(&(
        &draft.description,
        &draft.facts,
        &draft.documents,
        (!draft.complete).then_some(&draft.source.binding_id),
    ))
    .map_err(|_| "无法整理资源内容")?;
    result.groups.push(AgentCatalogContentGroup {
        id: format!("resource-content-{:x}", Sha256::digest(identity)),
        description: draft.description,
        facts: draft.facts,
        documents: draft.documents,
        sources: vec![draft.source],
        complete: draft.complete,
        notes: draft.notes,
        comparison: None,
    });
    Ok(())
}

fn shared_source(agent_kind: Option<AgentCliKind>) -> AgentCatalogContentSource {
    AgentCatalogContentSource {
        binding_id: None,
        agent_kind,
        scope: None,
        label: None,
        path: None,
        files: Vec::new(),
    }
}

fn shared_content(
    definition: AgentCatalogDefinition,
    mcp_connection: Option<serde_json::Value>,
) -> Result<Vec<ContentDraft>, String> {
    let document = |key: &str, label: &str, format, text| AgentCatalogContentDocument {
        key: key.to_owned(),
        label: label.to_owned(),
        format,
        text,
    };
    let shared = |document: AgentCatalogContentDocument,
                  kind: Option<AgentCliKind>,
                  mut facts: Vec<AgentResourceContentFact>| {
        let mut resource = AgentResourceContent {
            asset_id: definition.asset_id.clone(),
            category: definition.category,
            description: None,
            facts: Vec::new(),
        };
        let parsed = native_support::parse(&document.text, document.format).ok();
        resource_read::describe(
            &mut resource,
            parsed.as_ref(),
            &document.text,
            document.format,
        );
        if definition.category == AgentAssetCategory::Mcp {
            resource.description = None;
            if let Some(root) = &parsed {
                facts = mcp_facts(root);
            }
        }
        let mut source = shared_source(kind);
        source.files.push(AgentCatalogContentFile {
            key: document.key.clone(),
            target_id: format!("shared:{}:{kind:?}:{}", definition.asset_id, document.key),
            document_id: None,
            path: None,
            read_only_reason: None,
        });
        ContentDraft {
            description: resource.description,
            facts,
            documents: vec![document],
            source,
            complete: true,
            notes: Vec::new(),
        }
    };
    match definition.category {
        AgentAssetCategory::Skill => Ok(vec![shared(
            document(
                "body",
                "SKILL.md",
                AgentConfigurationFormat::Markdown,
                definition
                    .skill_markdown
                    .ok_or("共享定义中没有完整的 SKILL.md")?,
            ),
            None,
            Vec::new(),
        )]),
        AgentAssetCategory::Mcp => Ok(vec![shared(
            document(
                "definition",
                "MCP 连接配置",
                AgentConfigurationFormat::Json,
                serde_json::to_string_pretty(&mcp_connection.ok_or("共享定义缺少 MCP 连接配置")?)
                    .map_err(|_| "共享 MCP 配置无法读取")?,
            ),
            None,
            Vec::new(),
        )]),
        AgentAssetCategory::Hook => definition
            .hook
            .ok_or("共享定义缺少 Hook 配置")?
            .variants
            .into_iter()
            .map(|variant| {
                let root =
                    native_support::parse(&variant.group_json, AgentConfigurationFormat::Json)
                        .map_err(|error| error.message)?;
                let facts = hook_facts(&variant.event, &root);
                let text = DocumentSelection::Structured {
                    path: vec!["hooks".to_owned(), "0".to_owned()],
                    format: AgentConfigurationFormat::Json,
                }
                .text(&variant.group_json)
                .map_err(|error| error.message)?;
                Ok(shared(
                    document(
                        "definition",
                        "Hook 执行配置",
                        AgentConfigurationFormat::Json,
                        text,
                    ),
                    Some(variant.agent_kind),
                    facts,
                ))
            })
            .collect(),
        _ => Err("此资源没有可读取的共享定义".to_owned()),
    }
}

fn compare(
    before: &AgentCatalogContentGroup,
    after: &AgentCatalogContentGroup,
) -> AgentCatalogContentComparison {
    let keys = before
        .documents
        .iter()
        .chain(&after.documents)
        .map(|document| document.key.as_str())
        .collect::<BTreeSet<_>>();
    let documents = keys
        .into_iter()
        .filter_map(|key| {
            let left = before.documents.iter().find(|document| document.key == key);
            let right = after.documents.iter().find(|document| document.key == key);
            (left != right).then(|| AgentCatalogContentDocumentChange {
                key: key.to_owned(),
                comparable: left.is_some() && right.is_some() || before.complete && after.complete,
            })
        })
        .collect();
    let labels = before
        .facts
        .iter()
        .chain(&after.facts)
        .map(|fact| fact.label.as_str())
        .collect::<BTreeSet<_>>();
    let facts = labels
        .into_iter()
        .filter_map(|label| {
            let left = before
                .facts
                .iter()
                .find(|fact| fact.label == label)
                .map(|fact| fact.value.clone());
            let right = after
                .facts
                .iter()
                .find(|fact| fact.label == label)
                .map(|fact| fact.value.clone());
            (left != right).then(|| AgentCatalogContentFactChange {
                label: label.to_owned(),
                before: left,
                after: right,
            })
        })
        .collect();
    AgentCatalogContentComparison {
        documents,
        facts,
        description_changed: before.description != after.description,
    }
}

fn mcp_facts(connection: &serde_json::Value) -> Vec<AgentResourceContentFact> {
    let mut facts = Vec::new();
    add_string_fact(&mut facts, "命令", connection.get("command"));
    add_string_fact(&mut facts, "地址", connection.get("url"));
    add_string_fact(&mut facts, "连接方式", connection.get("type"));
    facts
}
