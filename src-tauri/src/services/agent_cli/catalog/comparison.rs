//! Read-only, bounded comparison. Public previews are never identity evidence.
use super::{
    definition::{DefinitionPayload, McpDefinition, PackageFile},
    hook_definition,
    observation::DefinitionReader,
    repository::Entry,
};
use crate::{models::*, services::agent_cli::environment::mutation::MutationInventory};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

const DOCUMENT_BYTES: usize = 64 * 1024;
const PUBLIC_BYTES: usize = 512 * 1024;
const PRIVATE_BYTES: usize = 64 * 1024 * 1024;

struct Side {
    public: AgentCatalogComparisonSide,
    // Complete payloads determine equality; bounded documents are display-only.
    // Internal comparison keys remain in this module.
    definitions: BTreeMap<String, Arc<DefinitionPayload>>,
}

pub(super) fn compare(
    snapshot: &MutationInventory,
    left: (&AgentCatalogAsset, &Entry),
    right: (&AgentCatalogAsset, &Entry),
) -> (
    Vec<AgentCatalogComparisonSide>,
    AgentCatalogComparisonEquality,
    Vec<AgentCatalogDifference>,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut remaining = PRIVATE_BYTES;
    let mut reader = DefinitionReader::new(snapshot);
    let left = side(&mut reader, left, deadline, &mut remaining);
    let right = side(&mut reader, right, deadline, &mut remaining);
    let complete = left.public.complete && right.public.complete;
    let equality = if !complete {
        AgentCatalogComparisonEquality::Unknown
    } else if left.definitions.keys().eq(right.definitions.keys()) {
        AgentCatalogComparisonEquality::Equal
    } else {
        AgentCatalogComparisonEquality::Different
    };
    let mut differences = if !complete {
        vec![AgentCatalogDifference {
            path: "完整内容".to_owned(),
            kind: AgentCatalogDifferenceKind::Unknown,
            left_summary: left.public.reason.clone(),
            right_summary: right.public.reason.clone(),
            reason: Some(
                "部分来源未能完整读取，不能确认两份资源是否相同；请查看对应来源的原因后刷新"
                    .to_owned(),
            ),
        }]
    } else if equality == AgentCatalogComparisonEquality::Equal {
        Vec::new()
    } else if left.definitions.len() == 1 && right.definitions.len() == 1 {
        definition_differences(
            left.definitions.first_key_value().unwrap().1,
            right.definitions.first_key_value().unwrap().1,
        )
    } else {
        vec![AgentCatalogDifference {
            path: "完整版本集合".to_owned(),
            kind: AgentCatalogDifferenceKind::Changed,
            left_summary: Some(format!("{} 份不同的完整内容", left.definitions.len())),
            right_summary: Some(format!("{} 份不同的完整内容", right.definitions.len())),
            reason: Some("共享版本或原生来源之间存在内容差异，请逐项核对下面的来源；未任选一份作为其余来源的代表".to_owned()),
        }]
    };
    let mut sides = vec![left.public, right.public];
    bound_public_output(&mut sides, &mut differences);
    (sides, equality, differences)
}

fn side(
    reader: &mut DefinitionReader<'_>,
    (asset, entry): (&AgentCatalogAsset, &Entry),
    deadline: Instant,
    remaining: &mut usize,
) -> Side {
    let mut result = Side {
        public: AgentCatalogComparisonSide {
            asset_id: asset.id.clone(),
            name: asset.name.clone(),
            category: asset.category,
            ownership: asset.ownership,
            version: asset.version,
            complete: true,
            reason: None,
            definition: Vec::new(),
            bindings: Vec::new(),
        },
        definitions: BTreeMap::new(),
    };
    if let Some(current) = entry.current() {
        let bytes = current.payload.comparison_bytes();
        if bytes > *remaining {
            *remaining = 0;
            result.public.complete = false;
            result.public.reason =
                Some("完整定义比较达到 64 MiB 预算，共享版本尚未参与本次比较".to_owned());
        } else if Instant::now() >= deadline {
            result.public.complete = false;
            result.public.reason =
                Some("完整定义比较达到 10 秒预算，共享版本尚未参与本次比较".to_owned());
        } else {
            *remaining -= bytes;
            result.public.definition = documents(&current.payload);
            result.definitions.insert(
                current.payload.fingerprint(),
                Arc::new(current.payload.clone()),
            );
        }
    }
    for binding in &asset.bindings {
        let observed = reader.observe(&binding.native, deadline, remaining);
        let mut public = AgentCatalogComparisonBinding {
            binding_id: binding.id.clone(),
            agent_kind: binding.native.agent_kind,
            context_id: binding.native.context_id.clone(),
            scope: binding.native.scope,
            path: observed.source_path.or_else(|| binding.native.path.clone()),
            provenance: binding.native.provenance.clone(),
            complete: observed.payload.is_ok(),
            reason: observed.payload.as_ref().err().cloned(),
            documents: Vec::new(),
        };
        match observed.payload {
            Ok(payload) => {
                public.documents = documents(&payload);
                result.definitions.insert(payload.fingerprint(), payload);
            }
            Err(_) => result.public.complete = false,
        }
        result.public.bindings.push(public);
    }
    if !asset.unresolved_targets.is_empty() {
        result.public.complete = false;
        result.public.reason = Some(
            "部分已应用范围尚未取得当前完整内容；已保留共享版本，请先刷新对应 Agent 的使用范围"
                .to_owned(),
        );
    } else if !result.public.complete {
        result
            .public
            .reason
            .get_or_insert_with(|| "部分原生来源无法完整读取，请查看下方的具体原因".to_owned());
    } else if result.definitions.is_empty() {
        result.public.complete = false;
        result.public.reason = Some("没有可比较的完整共享版本或原生定义".to_owned());
    }
    result
}

fn document(
    key: String,
    label: String,
    path: Option<String>,
    format: AgentCatalogComparisonFormat,
    content: Option<String>,
    reason: Option<String>,
) -> AgentCatalogComparisonDocument {
    AgentCatalogComparisonDocument {
        key,
        label,
        path,
        format,
        content,
        truncated: false,
        executable: None,
        reason,
    }
}

fn documents(payload: &DefinitionPayload) -> Vec<AgentCatalogComparisonDocument> {
    match payload {
        DefinitionPayload::Mcp(value) => {
            vec![document(
                "mcp/connection".to_owned(),
                "MCP 连接配置".to_owned(),
                None,
                AgentCatalogComparisonFormat::Json,
                serde_json::to_string_pretty(&value.connection_document()).ok(),
                Some("按连接字段比较，忽略文件格式及 Agent 本地策略".to_owned()),
            )]
        }
        DefinitionPayload::Skill(files) => files
            .iter()
            .map(|(path, file)| {
                let text = std::str::from_utf8(&file.bytes)
                    .ok()
                    .filter(|text| !text.contains('\0'));
                let mut value = document(
                    format!("skill/{path}"),
                    path.clone(),
                    Some(path.clone()),
                    if text.is_none() {
                        AgentCatalogComparisonFormat::Binary
                    } else if path.ends_with(".md") {
                        AgentCatalogComparisonFormat::Markdown
                    } else {
                        AgentCatalogComparisonFormat::Text
                    },
                    text.map(str::to_owned),
                    Some(file_summary(file)),
                );
                value.executable = Some(file.executable);
                value
            })
            .collect(),
        DefinitionPayload::Hook(values) => values
            .iter()
            .map(|(kind, value)| {
                let label = crate::services::agent_cli::definition(*kind).label;
                document(
                    format!("hook/{label}"),
                    format!("{label} · Hook"),
                    None,
                    AgentCatalogComparisonFormat::Json,
                    serde_json::to_string_pretty(value).ok(),
                    Some("包含触发事件和完整原生 Hook 规则".to_owned()),
                )
            })
            .collect(),
    }
}

fn file_summary(file: &PackageFile) -> String {
    format!(
        "{} 字节；{}",
        file.bytes.len(),
        if file.executable {
            "可执行"
        } else {
            "不可执行"
        }
    )
}

fn definition_differences(
    left: &DefinitionPayload,
    right: &DefinitionPayload,
) -> Vec<AgentCatalogDifference> {
    match (left, right) {
        (DefinitionPayload::Skill(left), DefinitionPayload::Skill(right)) => left
            .keys()
            .chain(right.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|path| {
                let before = left.get(path);
                let after = right.get(path);
                if before.zip(after).is_some_and(|(before, after)| {
                    before.bytes == after.bytes && before.executable == after.executable
                }) {
                    return None;
                }
                let reason = match (before, after) {
                    (Some(before), Some(after)) if before.bytes == after.bytes => {
                        "文件执行权限不同"
                    }
                    (Some(before), Some(after)) if before.executable == after.executable => {
                        "文件内容不同"
                    }
                    (Some(_), Some(_)) => "文件内容和执行权限均不同",
                    (None, _) => "仅右侧包含此文件",
                    (_, None) => "仅左侧包含此文件",
                };
                Some(AgentCatalogDifference {
                    path: path.clone(),
                    kind: if before.is_none() {
                        AgentCatalogDifferenceKind::Added
                    } else if after.is_none() {
                        AgentCatalogDifferenceKind::Removed
                    } else {
                        AgentCatalogDifferenceKind::Changed
                    },
                    left_summary: before.map(file_summary),
                    right_summary: after.map(file_summary),
                    reason: Some(reason.to_owned()),
                })
            })
            .collect(),
        (DefinitionPayload::Mcp(left), DefinitionPayload::Mcp(right)) => {
            mcp_differences(left, right)
        }
        (DefinitionPayload::Hook(left), DefinitionPayload::Hook(right)) => left
            .keys()
            .chain(right.keys())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .filter_map(|kind| {
                let before = left.get(kind);
                let after = right.get(kind);
                if before.zip(after).is_some_and(|(before, after)| {
                    hook_definition::fingerprint(before) == hook_definition::fingerprint(after)
                }) {
                    return None;
                }
                Some(AgentCatalogDifference {
                    path: format!(
                        "Hook/{}",
                        crate::services::agent_cli::definition(*kind).label
                    ),
                    kind: if before.is_none() {
                        AgentCatalogDifferenceKind::Added
                    } else if after.is_none() {
                        AgentCatalogDifferenceKind::Removed
                    } else {
                        AgentCatalogDifferenceKind::Changed
                    },
                    left_summary: before.map(|value| format!("事件 {}", value.event)),
                    right_summary: after.map(|value| format!("事件 {}", value.event)),
                    reason: Some("该 Agent 的原生 Hook 定义不同".to_owned()),
                })
            })
            .collect(),
        _ => vec![AgentCatalogDifference {
            path: "资源类型".to_owned(),
            kind: AgentCatalogDifferenceKind::Changed,
            left_summary: None,
            right_summary: None,
            reason: Some("资源类别不同，不能作为同一份定义比较".to_owned()),
        }],
    }
}

fn mcp_differences(left: &McpDefinition, right: &McpDefinition) -> Vec<AgentCatalogDifference> {
    let left = left.connection_document();
    let right = right.connection_document();
    [
        ("type", "连接方式"),
        ("command", "命令"),
        ("args", "参数"),
        ("url", "服务地址"),
        ("cwd", "工作目录"),
        ("env", "环境变量"),
        ("headers", "请求头"),
        ("connectionOptions", "认证与运行环境"),
    ]
    .into_iter()
    .filter(|(key, _)| left.get(*key) != right.get(*key))
    .map(|(_, label)| AgentCatalogDifference {
        path: format!("MCP/{label}"),
        kind: AgentCatalogDifferenceKind::Changed,
        left_summary: None,
        right_summary: None,
        reason: Some(format!("{label}不同")),
    })
    .collect()
}

/// Bound the serialized original text, accounting for JSON escapes and UTF-8.
fn clip_json_string(text: &mut String, maximum: usize) -> bool {
    let mut bytes = 2; // serialized string quotes
    let mut end = 0;
    for (offset, character) in text.char_indices() {
        let cost = match character {
            '"' | '\\' | '\n' | '\r' | '\t' | '\u{0008}' | '\u{000c}' => 2,
            character if character <= '\u{001f}' => 6,
            character => character.len_utf8(),
        };
        if bytes + cost > maximum {
            break;
        }
        bytes += cost;
        end = offset + character.len_utf8();
    }
    let truncated = end < text.len();
    text.truncate(end);
    truncated
}

fn all_documents(
    sides: &mut [AgentCatalogComparisonSide],
) -> impl Iterator<Item = &mut AgentCatalogComparisonDocument> {
    sides.iter_mut().flat_map(|side| {
        side.definition.iter_mut().chain(
            side.bindings
                .iter_mut()
                .flat_map(|binding| &mut binding.documents),
        )
    })
}

fn serialized_size(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

fn bound_public_output(
    sides: &mut [AgentCatalogComparisonSide],
    differences: &mut Vec<AgentCatalogDifference>,
) {
    for document in all_documents(sides) {
        if let Some(content) = &mut document.content {
            if clip_json_string(content, DOCUMENT_BYTES) {
                document.truncated = true;
                document.reason = Some(format!(
                    "{}；单份预览最多 64 KiB，显示内容已截断；完整内容仍参与比较",
                    document.reason.as_deref().unwrap_or_default()
                ));
            }
        }
    }
    // Account for the wire payload once, then update only changed documents.
    // Re-serializing the entire response per file becomes quadratic for large
    // manifests. Every binding and its source/scope metadata stays present.
    let mut size = serialized_size(&(&*sides, &*differences));
    let mut documents = all_documents(sides)
        .filter(|document| {
            document
                .content
                .as_ref()
                .is_some_and(|text| !text.is_empty())
        })
        .map(|document| (serialized_size(document), document))
        .collect::<Vec<_>>();
    documents.sort_by_key(|(bytes, _)| std::cmp::Reverse(*bytes));
    for (before, document) in documents {
        if size <= PUBLIC_BYTES {
            break;
        }
        let content = document.content.as_mut().unwrap();
        let maximum = serialized_size(content).saturating_sub(size - PUBLIC_BYTES + 512);
        clip_json_string(content, maximum);
        document.truncated = true;
        document.reason = Some("总预览最多 512 KiB，显示内容已截断；完整内容仍参与比较".to_owned());
        size = size
            .saturating_sub(before)
            .saturating_add(serialized_size(document));
    }
    if size <= PUBLIC_BYTES {
        return;
    }
    // Manifests without previewable text also count towards the response. Keep
    // their source records and explain each omitted file list beside its source.
    let mut manifests = sides
        .iter_mut()
        .flat_map(|side| {
            std::iter::once((&mut side.definition, &mut side.reason)).chain(
                side.bindings
                    .iter_mut()
                    .map(|binding| (&mut binding.documents, &mut binding.reason)),
            )
        })
        .filter(|(documents, _)| !documents.is_empty())
        .map(|(documents, reason)| (serialized_size(&(&*documents, &*reason)), documents, reason))
        .collect::<Vec<_>>();
    manifests.sort_by_key(|(bytes, _, _)| std::cmp::Reverse(*bytes));
    for (before, documents, reason) in manifests {
        if size <= PUBLIC_BYTES {
            break;
        }
        documents.clear();
        *reason = Some(format!(
            "{}预览超过 512 KiB 展示预算；完整内容仍参与比较",
            reason
                .as_ref()
                .map_or(String::new(), |reason| format!("{reason}；"))
        ));
        size = size
            .saturating_sub(before)
            .saturating_add(serialized_size(&(documents, reason)));
    }
    if size > PUBLIC_BYTES && differences.len() > 1 {
        differences.truncate(1);
        differences[0].reason = Some(
            "详细差异超过 512 KiB 展示预算，请缩小来源范围后重新比较；完整内容结论保持不变"
                .to_owned(),
        );
    }
}

#[cfg(test)]
#[path = "comparison_tests.rs"]
mod tests;
